//! iroh transport for the metadata document (PLAN §7 F1 layer 2).
//!
//! One sync session is one QUIC bidirectional stream on [`SYNC_ALPN`]:
//!
//! 1. client → host: [`Hello`] — own address, own document version and, when
//!    pairing, the one-time secret from the QR code;
//! 2. host → client: [`Reply`] — `Denied`, or its version plus the updates
//!    the client lacks;
//! 3. client → host: [`Push`] — the updates the host lacks;
//! 4. host → client: [`Reply::Done`].
//!
//! Only version vectors and CRDT updates are frames; there is no message
//! that names, requests or carries a file, so originals cannot travel
//! (PLAN §7 F1: "originals are not synced by default"). Frame sizes are
//! capped so a misbehaving peer cannot stream anything large either.
//! iroh authenticates the remote endpoint id during the handshake, which is
//! what the paired-device check relies on.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId};
use iroh_tickets::{endpoint::EndpointTicket, Ticket};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::pairing::{Token, TOKEN_LEN};
use crate::{MetadataDoc, PairedDevices, PairingTicket, SyncError};

/// ALPN of the metadata sync protocol.
pub const SYNC_ALPN: &[u8] = b"fotobase/sync/1";

/// Cap on the first, still unauthenticated frame.
const MAX_HELLO: usize = 64 * 1024;
/// Cap on a frame of CRDT updates.
const MAX_UPDATES: usize = 256 * 1024 * 1024;
const MAX_REPLY_HEADER: usize = MAX_UPDATES + MAX_HELLO;
const SESSION_TIMEOUT: Duration = Duration::from_secs(120);
const CLOSE_GRACE: Duration = Duration::from_secs(5);
const DEFAULT_TICKET_LIFETIME: Duration = Duration::from_secs(10 * 60);

const CLOSE_OK: u32 = 0;
const CLOSE_DENIED: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Hello {
    token: Option<Token>,
    /// The sender's own address as an iroh endpoint ticket, so the host can
    /// reach it back later.
    addr: Vec<u8>,
    version: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
enum Reply {
    Denied,
    Accepted { version: Vec<u8>, updates: Vec<u8> },
    Done,
}

#[derive(Serialize, Deserialize)]
struct Push {
    updates: Vec<u8>,
}

fn transport_err(e: impl fmt::Display) -> SyncError {
    SyncError::Transport(e.to_string())
}

async fn write_frame<T: Serialize>(send: &mut SendStream, msg: &T) -> Result<(), SyncError> {
    let body = postcard::to_stdvec(msg).map_err(transport_err)?;
    let len = u32::try_from(body.len()).map_err(transport_err)?;
    send.write_all(&len.to_be_bytes()).await.map_err(transport_err)?;
    send.write_all(&body).await.map_err(transport_err)
}

async fn read_frame<T: for<'de> Deserialize<'de>>(
    recv: &mut RecvStream,
    max: usize,
) -> Result<T, SyncError> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await.map_err(transport_err)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > max {
        return Err(SyncError::Transport(format!(
            "frame of {len} bytes exceeds the {max} byte limit"
        )));
    }
    let mut body = vec![0u8; len];
    recv.read_exact(&mut body).await.map_err(transport_err)?;
    postcard::from_bytes(&body).map_err(transport_err)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// State shared by the protocol handler, the node and background sync.
struct Shared {
    endpoint: Endpoint,
    doc: Arc<Mutex<MetadataDoc>>,
    paired: Mutex<PairedDevices>,
    /// Outstanding one-time pairing secrets and when they lapse.
    tickets: Mutex<HashMap<Token, Instant>>,
    sessions_started: AtomicU64,
}

impl Shared {
    fn redeem(&self, token: &Token) -> bool {
        let mut tickets = lock(&self.tickets);
        let now = Instant::now();
        tickets.retain(|_, expires| *expires > now);
        tickets.remove(token).is_some()
    }

    /// Host side of one session.
    async fn serve(&self, conn: &Connection) -> Result<(), SyncError> {
        let remote = conn.remote_id();
        let (mut send, mut recv) = conn.accept_bi().await.map_err(transport_err)?;
        let hello: Hello = read_frame(&mut recv, MAX_HELLO).await?;

        let known = lock(&self.paired).contains(&remote);
        let authorized = known || hello.token.is_some_and(|t| self.redeem(&t));
        if !authorized {
            write_frame(&mut send, &Reply::Denied).await?;
            send.finish().map_err(transport_err)?;
            let _ = tokio::time::timeout(CLOSE_GRACE, conn.closed()).await;
            return Err(SyncError::Unauthorized);
        }

        let addr = EndpointTicket::decode_bytes(&hello.addr).ok().map(EndpointAddr::from);
        lock(&self.paired).insert(remote, addr)?;

        let (version, updates) = {
            let doc = lock(&self.doc);
            (doc.version(), doc.export_updates_since(&hello.version)?)
        };
        write_frame(&mut send, &Reply::Accepted { version, updates }).await?;

        let push: Push = read_frame(&mut recv, MAX_UPDATES).await?;
        lock(&self.doc).import(&push.updates)?;
        write_frame(&mut send, &Reply::Done).await?;
        send.finish().map_err(transport_err)?;
        let _ = tokio::time::timeout(CLOSE_GRACE, conn.closed()).await;
        Ok(())
    }

    /// Client side of one session; `pairing` carries the ticket the first
    /// time two devices meet.
    async fn sync_session(
        &self,
        addr: EndpointAddr,
        pairing: Option<Token>,
    ) -> Result<EndpointId, SyncError> {
        self.sessions_started.fetch_add(1, Ordering::SeqCst);
        let remote = addr.id;
        let conn = self.endpoint.connect(addr.clone(), SYNC_ALPN).await.map_err(transport_err)?;
        let result = self.run_client(&conn, pairing).await;
        match &result {
            Ok(()) => conn.close(CLOSE_OK.into(), b"done"),
            Err(SyncError::Unauthorized) => conn.close(CLOSE_DENIED.into(), b"denied"),
            Err(_) => conn.close(2u32.into(), b"error"),
        }
        result?;
        lock(&self.paired).insert(remote, Some(addr))?;
        Ok(remote)
    }

    async fn run_client(&self, conn: &Connection, pairing: Option<Token>) -> Result<(), SyncError> {
        let (mut send, mut recv) = conn.open_bi().await.map_err(transport_err)?;
        let hello = Hello {
            token: pairing,
            addr: EndpointTicket::new(self.endpoint.addr()).encode_bytes(),
            version: lock(&self.doc).version(),
        };
        write_frame(&mut send, &hello).await?;

        match read_frame::<Reply>(&mut recv, MAX_REPLY_HEADER).await {
            Ok(Reply::Accepted { version, updates }) => {
                let push = {
                    let mut doc = lock(&self.doc);
                    doc.import(&updates)?;
                    Push { updates: doc.export_updates_since(&version)? }
                };
                write_frame(&mut send, &push).await?;
                send.finish().map_err(transport_err)?;
            }
            Ok(Reply::Denied) => return Err(SyncError::Unauthorized),
            Ok(Reply::Done) => return Err(SyncError::Transport("unexpected reply".into())),
            Err(e) => return Err(e),
        }
        match read_frame::<Reply>(&mut recv, MAX_HELLO).await? {
            Reply::Done => Ok(()),
            _ => Err(SyncError::Transport("unexpected reply".into())),
        }
    }

    async fn sync_with(&self, id: EndpointId) -> Result<(), SyncError> {
        let addr = lock(&self.paired).addr(&id);
        if !lock(&self.paired).contains(&id) {
            return Err(SyncError::Unauthorized);
        }
        tokio::time::timeout(SESSION_TIMEOUT, self.sync_session(addr, None))
            .await
            .map_err(|_| SyncError::Transport("sync timed out".into()))??;
        Ok(())
    }

    async fn sync_all(&self) -> Vec<(EndpointId, Result<(), SyncError>)> {
        let ids = lock(&self.paired).ids();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            out.push((id, self.sync_with(id).await));
        }
        out
    }
}

struct Handler(Arc<Shared>);

impl fmt::Debug for Handler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SyncHandler")
    }
}

impl ProtocolHandler for Handler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        match tokio::time::timeout(SESSION_TIMEOUT, self.0.serve(&connection)).await {
            Ok(Ok(())) => {}
            Ok(Err(SyncError::Unauthorized)) => {
                tracing::warn!(peer = %connection.remote_id().fmt_short(), "refused sync from unpaired device");
            }
            Ok(Err(e)) => {
                tracing::warn!(peer = %connection.remote_id().fmt_short(), error = %e, "sync session failed")
            }
            Err(_) => tracing::warn!("sync session timed out"),
        }
        Ok(())
    }
}

/// A device taking part in sync: serves the document to paired devices and
/// pulls from them.
pub struct SyncNode {
    shared: Arc<Shared>,
    router: Router,
}

impl SyncNode {
    /// Start serving `doc` on an already bound iroh endpoint. The caller
    /// picks relay servers and the device key when binding the endpoint
    /// (the default relays are n0's public ones; the endpoint needs no
    /// address-lookup service because tickets carry the relay URL).
    pub fn spawn(endpoint: Endpoint, doc: Arc<Mutex<MetadataDoc>>, paired: PairedDevices) -> Self {
        let shared = Arc::new(Shared {
            endpoint: endpoint.clone(),
            doc,
            paired: Mutex::new(paired),
            tickets: Mutex::new(HashMap::new()),
            sessions_started: AtomicU64::new(0),
        });
        let router = Router::builder(endpoint).accept(SYNC_ALPN, Handler(shared.clone())).spawn();
        Self { shared, router }
    }

    /// This device's iroh endpoint id.
    pub fn id(&self) -> EndpointId {
        self.shared.endpoint.id()
    }

    /// A ticket for a second device to pair with this one, valid for ten
    /// minutes and a single use. Show it as a QR code.
    pub fn pairing_ticket(&self) -> PairingTicket {
        self.pairing_ticket_valid_for(DEFAULT_TICKET_LIFETIME)
    }

    pub fn pairing_ticket_valid_for(&self, lifetime: Duration) -> PairingTicket {
        let mut token = [0u8; TOKEN_LEN];
        getrandom::fill(&mut token).expect("operating system random source");
        lock(&self.shared.tickets).insert(token, Instant::now() + lifetime);
        PairingTicket::new(self.shared.endpoint.addr(), token)
    }

    /// Pair with the device that showed `ticket` and run a first sync.
    pub async fn pair(&self, ticket: &PairingTicket) -> Result<EndpointId, SyncError> {
        let session =
            self.shared.sync_session(ticket.endpoint_addr().clone(), Some(ticket.token()));
        tokio::time::timeout(SESSION_TIMEOUT, session)
            .await
            .map_err(|_| SyncError::Transport("pairing timed out".into()))?
    }

    pub fn paired_devices(&self) -> Vec<EndpointId> {
        lock(&self.shared.paired).ids()
    }

    /// Forget a device; it can no longer sync until paired again.
    pub fn unpair(&self, id: &EndpointId) -> Result<bool, SyncError> {
        lock(&self.shared.paired).remove(id)
    }

    /// Exchange updates with one paired device.
    pub async fn sync_with(&self, id: EndpointId) -> Result<(), SyncError> {
        self.shared.sync_with(id).await
    }

    /// Exchange updates with every paired device, one after another; one
    /// unreachable device does not stop the others.
    pub async fn sync_all(&self) -> Vec<(EndpointId, Result<(), SyncError>)> {
        self.shared.sync_all().await
    }

    /// How many outgoing sessions this node has started (pairing included).
    pub fn sessions_started(&self) -> u64 {
        self.shared.sessions_started.load(Ordering::SeqCst)
    }

    /// Sync with paired devices in the background; see [`BackgroundSync`].
    pub fn spawn_background(&self, config: BackgroundConfig) -> BackgroundSync {
        BackgroundSync::spawn(self.shared.clone(), config)
    }

    pub async fn shutdown(self) {
        if let Err(e) = self.router.shutdown().await {
            tracing::warn!(error = %e, "sync router did not shut down cleanly");
        }
    }
}

/// Lets the app hold sync back when it would cost too much — metered mobile
/// data, low battery (PLAN §3: throttle on battery).
pub trait SyncPolicy: Send + Sync + 'static {
    fn allow_sync(&self) -> bool;
}

/// Sync whenever asked.
#[derive(Debug, Clone, Copy, Default)]
pub struct AlwaysSync;

impl SyncPolicy for AlwaysSync {
    fn allow_sync(&self) -> bool {
        true
    }
}

pub struct BackgroundConfig {
    /// Quiet time after the last change before syncing, so a burst of edits
    /// goes out as one session per device.
    pub debounce: Duration,
    /// Pull from paired devices this often even without local changes.
    pub interval: Duration,
    pub policy: Arc<dyn SyncPolicy>,
}

impl Default for BackgroundConfig {
    fn default() -> Self {
        Self {
            debounce: Duration::from_secs(5),
            interval: Duration::from_secs(15 * 60),
            policy: Arc::new(AlwaysSync),
        }
    }
}

/// Background task that syncs after local changes settle and on a slow
/// timer. Skipped rounds (policy says no) are retried on the next change or
/// tick.
pub struct BackgroundSync {
    changed: Arc<Notify>,
    stop: Arc<Notify>,
    task: JoinHandle<()>,
}

impl BackgroundSync {
    fn spawn(shared: Arc<Shared>, config: BackgroundConfig) -> Self {
        let changed = Arc::new(Notify::new());
        let stop = Arc::new(Notify::new());
        let task = tokio::spawn(run_background(shared, config, changed.clone(), stop.clone()));
        Self { changed, stop, task }
    }

    /// Tell the task the local document changed.
    pub fn notify_changed(&self) {
        self.changed.notify_one();
    }

    pub async fn shutdown(self) {
        self.stop.notify_one();
        let _ = self.task.await;
    }
}

async fn run_background(
    shared: Arc<Shared>,
    config: BackgroundConfig,
    changed: Arc<Notify>,
    stop: Arc<Notify>,
) {
    let mut tick =
        tokio::time::interval_at(tokio::time::Instant::now() + config.interval, config.interval);
    loop {
        tokio::select! {
            _ = stop.notified() => return,
            _ = tick.tick() => {}
            _ = changed.notified() => {
                // Wait for the burst to end.
                loop {
                    tokio::select! {
                        _ = stop.notified() => return,
                        _ = changed.notified() => continue,
                        _ = tokio::time::sleep(config.debounce) => break,
                    }
                }
            }
        }
        if !config.policy.allow_sync() {
            continue;
        }
        for (id, result) in shared.sync_all().await {
            if let Err(e) = result {
                tracing::debug!(peer = %id.fmt_short(), error = %e, "background sync failed");
            }
        }
    }
}
