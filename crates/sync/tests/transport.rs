//! TASK-10-01-04 integration: iroh transport and QR pairing between two
//! local peers, over a direct path and over a relay-only path (PLAN §7 F1
//! layer 2). A local test relay stands in for the public n0 relays.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fotobase_sync::{
    BackgroundConfig, MetadataDoc, PairedDevices, PairingTicket, PhotoMeta, SyncError, SyncNode,
    SyncPolicy, SYNC_ALPN,
};
use iroh::endpoint::presets;
use iroh::test_utils::run_relay_server;
use iroh::tls::CaTlsConfig;
use iroh::{Endpoint, RelayMap, RelayMode};

const KEY: &str = "sha256:0f1e2d";

type SharedDoc = Arc<Mutex<MetadataDoc>>;

async fn endpoint(relay: &RelayMap, direct: bool) -> Endpoint {
    let mut builder = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Custom(relay.clone()))
        .ca_tls_config(CaTlsConfig::insecure_skip_verify());
    if !direct {
        builder = builder.clear_ip_transports();
    }
    let ep = builder.bind().await.expect("bind endpoint");
    ep.online().await;
    ep
}

fn doc_with(peer: u64, rating: Option<u8>) -> SharedDoc {
    let mut doc = MetadataDoc::with_peer(peer);
    if let Some(rating) = rating {
        doc.update_photo(KEY, &PhotoMeta { rating: Some(rating), ..Default::default() }).unwrap();
    }
    Arc::new(Mutex::new(doc))
}

fn rating(doc: &SharedDoc) -> Option<u8> {
    doc.lock().unwrap().photo(KEY).and_then(|m| m.rating)
}

fn set_caption(doc: &SharedDoc, caption: &str) {
    let mut doc = doc.lock().unwrap();
    let mut meta = doc.photo(KEY).unwrap_or_default();
    meta.caption = Some(caption.to_string());
    doc.update_photo(KEY, &meta).unwrap();
}

fn caption(doc: &SharedDoc) -> Option<String> {
    doc.lock().unwrap().photo(KEY).and_then(|m| m.caption)
}

async fn node(relay: &RelayMap, direct: bool, doc: &SharedDoc) -> SyncNode {
    SyncNode::spawn(endpoint(relay, direct).await, doc.clone(), PairedDevices::new())
}

/// Pair `joiner` to `host` through the string a QR code would carry, and
/// check that the library crossed the wire.
async fn pair_through_qr(host: &SyncNode, joiner: &SyncNode) {
    let payload = host.pairing_ticket().to_qr_payload();
    let ticket = PairingTicket::from_qr_payload(&payload).expect("scanned ticket parses");
    let id = tokio::time::timeout(Duration::from_secs(30), joiner.pair(&ticket))
        .await
        .expect("pairing timed out")
        .expect("pairing succeeds");
    assert_eq!(id, host.id());
    assert!(host.paired_devices().contains(&joiner.id()));
    assert!(joiner.paired_devices().contains(&host.id()));
}

#[tokio::test(flavor = "multi_thread")]
async fn two_peers_pair_via_qr_payload_and_exchange_a_crdt_update() {
    let (relay, _url, _server) = run_relay_server().await.unwrap();
    let host_doc = doc_with(1, Some(4));
    let joiner_doc = doc_with(2, None);
    let host = node(&relay, true, &host_doc).await;
    let joiner = node(&relay, true, &joiner_doc).await;

    pair_through_qr(&host, &joiner).await;
    assert_eq!(rating(&joiner_doc), Some(4), "pairing syncs the host's metadata");

    // Later edits travel both ways, started from either side.
    set_caption(&joiner_doc, "from the joiner");
    joiner.sync_with(host.id()).await.unwrap();
    assert_eq!(caption(&host_doc).as_deref(), Some("from the joiner"));

    set_caption(&host_doc, "from the host");
    host.sync_with(joiner.id()).await.unwrap();
    assert_eq!(caption(&joiner_doc).as_deref(), Some("from the host"));

    host.shutdown().await;
    joiner.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pairing_completes_over_the_relay_when_direct_paths_are_blocked() {
    let (relay, _url, _server) = run_relay_server().await.unwrap();
    let host_doc = doc_with(1, Some(5));
    let joiner_doc = doc_with(2, None);
    // No IP transports: the only way between the peers is the relay.
    let host = node(&relay, false, &host_doc).await;
    let joiner = node(&relay, false, &joiner_doc).await;

    let ticket = host.pairing_ticket();
    assert!(
        ticket.endpoint_addr().ip_addrs().next().is_none(),
        "relay-only ticket carries no direct address"
    );
    assert!(ticket.endpoint_addr().relay_urls().next().is_some());

    pair_through_qr(&host, &joiner).await;
    assert_eq!(rating(&joiner_doc), Some(5));

    set_caption(&host_doc, "via relay");
    host.sync_with(joiner.id()).await.unwrap();
    assert_eq!(caption(&joiner_doc).as_deref(), Some("via relay"));

    host.shutdown().await;
    joiner.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pairing_ticket_works_once() {
    let (relay, _url, _server) = run_relay_server().await.unwrap();
    let host = node(&relay, true, &doc_with(1, Some(1))).await;
    let first = node(&relay, true, &doc_with(2, None)).await;
    let second_doc = doc_with(3, None);
    let second = node(&relay, true, &second_doc).await;

    let ticket = host.pairing_ticket();
    first.pair(&ticket).await.unwrap();
    let err = second.pair(&ticket).await.unwrap_err();
    assert!(matches!(err, SyncError::Unauthorized), "got {err:?}");
    assert_eq!(rating(&second_doc), None, "rejected peer learns nothing");
    assert!(!host.paired_devices().contains(&second.id()));
}

#[tokio::test(flavor = "multi_thread")]
async fn unpaired_peers_cannot_pull_metadata() {
    let (relay, _url, _server) = run_relay_server().await.unwrap();
    let host = node(&relay, true, &doc_with(1, Some(3))).await;
    let stranger_doc = doc_with(2, None);
    let stranger = node(&relay, true, &stranger_doc).await;

    // The stranger knows the host's address (say, from a leaked ticket
    // minus its token) but is not paired.
    let leaked = host.pairing_ticket();
    let forged = PairingTicket::new(leaked.endpoint_addr().clone(), [0u8; 16]);
    let err = stranger.pair(&forged).await.unwrap_err();
    assert!(matches!(err, SyncError::Unauthorized), "got {err:?}");
    assert_eq!(rating(&stranger_doc), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_tickets_are_refused() {
    let (relay, _url, _server) = run_relay_server().await.unwrap();
    let host = node(&relay, true, &doc_with(1, Some(2))).await;
    let joiner = node(&relay, true, &doc_with(2, None)).await;

    let ticket = host.pairing_ticket_valid_for(Duration::ZERO);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let err = joiner.pair(&ticket).await.unwrap_err();
    assert!(matches!(err, SyncError::Unauthorized), "got {err:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn paired_devices_survive_a_restart() {
    let (relay, _url, _server) = run_relay_server().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("paired.json");

    let host = node(&relay, true, &doc_with(1, Some(4))).await;
    let joiner_doc = doc_with(2, None);
    let joiner_ep = endpoint(&relay, true).await;
    let joiner_id = joiner_ep.id();
    let joiner =
        SyncNode::spawn(joiner_ep, joiner_doc.clone(), PairedDevices::open(&store).unwrap());
    joiner.pair(&host.pairing_ticket()).await.unwrap();
    joiner.shutdown().await;

    let reloaded = PairedDevices::open(&store).unwrap();
    assert_eq!(reloaded.ids(), vec![host.id()]);
    assert_ne!(joiner_id, host.id());
}

#[tokio::test(flavor = "multi_thread")]
async fn oversized_frames_are_refused_before_authentication() {
    let (relay, _url, _server) = run_relay_server().await.unwrap();
    let host = node(&relay, true, &doc_with(1, Some(1))).await;
    let rogue = endpoint(&relay, true).await;

    let conn =
        rogue.connect(host.pairing_ticket().endpoint_addr().clone(), SYNC_ALPN).await.unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    // Announce a 1 GiB first frame — the size of a photo, not of metadata.
    // Send no body: rejection must happen as soon as the length arrives.
    send.write_all(&(1u32 << 30).to_be_bytes()).await.unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(10), recv.read_to_end(1024))
        .await
        .expect("host did not reject the oversized frame before waiting for its body");
    assert!(!matches!(reply, Ok(ref bytes) if !bytes.is_empty()), "host answered: {reply:?}");
    assert!(host.paired_devices().is_empty());
}

#[test]
fn qr_code_renders_as_svg() {
    let id = iroh::SecretKey::generate().public();
    let ticket = PairingTicket::new(iroh::EndpointAddr::new(id), [7u8; 16]);
    let svg = ticket.to_qr_svg().unwrap();
    assert!(svg.contains("<svg"), "got {svg:.60}");
}

#[test]
fn garbage_is_not_a_ticket() {
    assert!(PairingTicket::from_qr_payload("https://example.com").is_err());
    assert!(PairingTicket::from_qr_payload("fbpairnot-base32!").is_err());
}

struct Gate(AtomicBool);

impl SyncPolicy for Gate {
    fn allow_sync(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn background_sync_batches_changes_and_honours_the_policy() {
    let (relay, _url, _server) = run_relay_server().await.unwrap();
    let host_doc = doc_with(1, Some(1));
    let joiner_doc = doc_with(2, None);
    let host = node(&relay, true, &host_doc).await;
    let joiner = node(&relay, true, &joiner_doc).await;
    pair_through_qr(&host, &joiner).await;
    let base = joiner.sessions_started();

    let gate = Arc::new(Gate(AtomicBool::new(false)));
    let background = joiner.spawn_background(BackgroundConfig {
        debounce: Duration::from_millis(200),
        interval: Duration::from_secs(3600),
        policy: gate.clone(),
    });

    // A burst of local edits while the policy says "not now" (metered link
    // or low battery) sends nothing.
    for i in 0..5 {
        set_caption(&joiner_doc, &format!("edit {i}"));
        background.notify_changed();
    }
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(joiner.sessions_started(), base, "policy blocks sync");
    assert_eq!(caption(&host_doc), None);

    // Once allowed, the whole burst goes out in one session.
    gate.0.store(true, Ordering::SeqCst);
    background.notify_changed();
    for _ in 0..100 {
        if caption(&host_doc).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(caption(&host_doc).as_deref(), Some("edit 4"));
    assert_eq!(joiner.sessions_started(), base + 1, "burst batched into one session");

    background.shutdown().await;
    host.shutdown().await;
    joiner.shutdown().await;
}
