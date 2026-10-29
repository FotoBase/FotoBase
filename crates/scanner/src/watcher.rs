//! `notify`-based watcher for "Scan Always" folders (TASK-01-007, PLAN §4).
//!
//! File events are debounced (hundreds of ms) so editor save-storms and
//! temp-file churn collapse into one incremental scan per quiet window, then
//! fed to a serial worker that runs [`scan_folder`] against the catalog.
//! Local folders only in P1 — network paths get a polling watcher in P1.5
//! (PLAN §4). The worker is serial and the catalog lock is held for the
//! duration of a scan, so no file is ever scanned twice concurrently; events
//! arriving mid-scan simply enqueue a follow-up scan.
//!
//! Paths: backends report canonical paths (`/var` vs `/private/var` on
//! macOS), so event paths are canonicalized for root matching, but the scan
//! always runs against the root **as given** — the catalog keys rows by the
//! caller's paths, and mixing the two spellings would duplicate rows.
//!
//! Lifecycle: roots can be added/removed at runtime; the Folder Manager's
//! Scan Always list (TASK-01-016) will drive these calls. The `jobs` queue
//! (TASK-01-009) will later own scan execution; until then the worker keeps
//! the same serial, cancellable contract.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use fotobase_catalog::Catalog;
use notify::Watcher as NotifyWatcher;

use crate::polling::{is_network_path, PollConfig, Poller};
use crate::scan_folder;
use crate::ScanReport;

/// Default debounce window ("hundreds of ms" per the task spec).
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(300);

/// Hook called (from the scan worker thread) after every watcher-driven
/// scan that changed the catalog. The app shell uses it to emit
/// `library-changed` so the UI re-lists without polling — a cold-start
/// scan (perf harness, or the first launch after adding a watched folder)
/// otherwise lands in a catalog the grid never re-reads.
pub type ScanHook = Arc<dyn Fn(&ScanReport) + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum WatcherError {
    #[error("watcher setup failed: {0}")]
    Setup(#[from] notify::Error),
    #[error("could not resolve watched path: {0}")]
    Path(#[from] io::Error),
    #[error("watcher has been stopped")]
    Stopped,
}

enum Cmd {
    /// `Add` carries a reply channel so `add_root` returns only after the
    /// root is watched (and its closing incremental scan is enqueued).
    Add(PathBuf, Sender<()>),
    /// `Remove` carries a reply channel so `remove_root` returns only after
    /// the root's watcher is dropped — otherwise a file written immediately
    /// after the call could still be picked up by the not-yet-removed watch.
    Remove(PathBuf, Sender<()>),
}

/// Which backend watches a root (TASK-02-008): `notify` for local
/// filesystems, polling for SMB/NFS where inotify/FSEvents never fire.
enum Backend {
    Notify(notify::RecommendedWatcher),
    Poll(Poller),
}

/// Canonicalize an event path; for vanished files fall back to
/// canonical-parent + file name (deletes report paths that no longer exist).
fn canonical_event_path(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok().or_else(|| {
        let parent = path.parent()?;
        let name = path.file_name()?;
        Some(fs::canonicalize(parent).ok()?.join(name))
    })
}

/// One notify watcher bound to one root: events under `canon` are forwarded
/// to `event_tx` as `given` (the root as the caller spelled it, which is what
/// the catalog keys rows by).
fn make_root_watcher(
    canon: PathBuf,
    given: PathBuf,
    event_tx: Sender<PathBuf>,
) -> Result<notify::RecommendedWatcher, notify::Error> {
    notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
        let Ok(ev) = res else { return };
        for path in ev.paths {
            let Some(c) = canonical_event_path(&path) else { continue };
            if c.starts_with(&canon) {
                let _ = event_tx.send(given.clone());
            }
        }
    })
}

/// Handle for a running watcher. Dropping it shuts the watcher down; `stop`
/// waits for the threads to exit.
pub struct Watcher {
    scan_count: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
    cmd_tx: Option<Sender<Cmd>>,
    event_tx: Option<Sender<PathBuf>>,
    work_tx: Option<Sender<PathBuf>>,
    threads: Vec<JoinHandle<()>>,
}

impl Watcher {
    /// Start watching `roots` recursively and rescanning them (incrementally,
    /// through [`scan_folder`]) into `catalog` after each debounce window.
    pub fn spawn(
        roots: &[PathBuf],
        catalog: Arc<Mutex<Catalog>>,
        debounce: Duration,
    ) -> Result<Self, WatcherError> {
        Self::spawn_with(roots, catalog, debounce, None)
    }

    /// [`spawn`] with a [`ScanHook`]: called after every watcher-driven scan
    /// that changed the catalog (added/updated/removed), from the scan
    /// worker thread. Emits must be thread-safe (Tauri's `AppHandle` is).
    pub fn spawn_with(
        roots: &[PathBuf],
        catalog: Arc<Mutex<Catalog>>,
        debounce: Duration,
        on_change: Option<ScanHook>,
    ) -> Result<Self, WatcherError> {
        let (event_tx, event_rx) = mpsc::channel::<PathBuf>();
        let (work_tx, work_rx) = mpsc::channel::<PathBuf>();
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let cancel = Arc::new(AtomicBool::new(false));
        let scan_count = Arc::new(AtomicUsize::new(0));

        let mut threads = Vec::new();

        // Watch thread: one backend per root. Local filesystems get a notify
        // watcher (one per root — recommending a shared watcher is
        // unreliable: on macOS the FSEvents backend restarts the whole
        // stream on every watch() call, losing or replaying events);
        // network-classified roots (SMB/NFS, where inotify/FSEvents never
        // fire) get the adaptive polling watcher (TASK-02-008). Both
        // backends feed the same event channel, so debounce and the serial
        // scan worker are unchanged.
        {
            let event_tx = event_tx.clone();
            let mut watchers: HashMap<PathBuf, Backend> = HashMap::new();
            for root in roots {
                let canon = fs::canonicalize(root)?;
                let given = root.clone();
                let mut backend = if is_network_path(&canon) {
                    Backend::Poll(Poller::spawn(
                        given.clone(),
                        event_tx.clone(),
                        PollConfig::default(),
                    ))
                } else {
                    Backend::Notify(make_root_watcher(canon.clone(), given, event_tx.clone())?)
                };
                if let Backend::Notify(w) = &mut backend {
                    w.watch(&canon, notify::RecursiveMode::Recursive)?;
                }
                watchers.insert(canon, backend);
            }
            threads.push(thread::Builder::new().name("fb-watch".into()).spawn(move || {
                for cmd in cmd_rx {
                    match cmd {
                        Cmd::Add(given, reply) => {
                            match fs::canonicalize(&given) {
                                Ok(canon) => {
                                    if is_network_path(&canon) {
                                        // Polling backend (TASK-02-008):
                                        // notify never fires over SMB/NFS.
                                        // The poller's first cycle establishes
                                        // its baseline; a closing incremental
                                        // scan through the debounce pipeline
                                        // closes the arming gap, same as for
                                        // notify.
                                        watchers.insert(
                                            canon,
                                            Backend::Poll(Poller::spawn(
                                                given.clone(),
                                                event_tx.clone(),
                                                PollConfig::default(),
                                            )),
                                        );
                                        let _ = event_tx.send(given.clone());
                                        let _ = reply.send(());
                                    } else {
                                        match make_root_watcher(
                                            canon.clone(),
                                            given.clone(),
                                            event_tx.clone(),
                                        ) {
                                            Ok(mut w) => {
                                                if w.watch(&canon, notify::RecursiveMode::Recursive).is_ok()
                                                {
                                                    watchers.insert(canon, Backend::Notify(w));
                                                    // Watcher arming is asynchronous on some
                                                    // backends (FSEvents schedules its runloop
                                                    // off-thread), so events in the arming gap
                                                    // can be missed. An immediate incremental
                                                    // scan through the debounce pipeline closes
                                                    // that gap; it is a fast no-op when nothing
                                                    // changed.
                                                    let _ = event_tx.send(given.clone());
                                                } else {
                                                    tracing::warn!(path = %given.display(), "cannot watch folder");
                                                }
                                            }
                                            Err(e) => tracing::warn!(
                                                path = %given.display(),
                                                error = %e,
                                                "cannot watch folder"
                                            ),
                                        }
                                        let _ = reply.send(());
                                    }
                                }
                                Err(_) => {
                                    let _ = reply.send(());
                                }
                            }
                        }
                        Cmd::Remove(given, reply) => {
                            if let Ok(canon) = fs::canonicalize(&given) {
                                // Dropping the watcher stops its stream; a
                                // poll backend is stopped explicitly so its
                                // thread joins (the notify watcher needs only
                                // to be dropped).
                                if let Some(Backend::Poll(poller)) = watchers.remove(&canon) {
                                    poller.stop();
                                }
                            }
                            // Ack after the watcher is dropped: past this point
                            // no further events for the root can be captured.
                            let _ = reply.send(());
                        }
                    }
                }
            })?);
        }

        // Debounce thread: collapse bursts of events into one work item per
        // root after a quiet window. The deadline rolls forward on every
        // event (backends deliver a burst like write-then-rename-in in
        // several waves hundreds of ms apart), capped at 2 s from the first
        // pending event so continuous churn still flushes.
        {
            let work_tx = work_tx.clone();
            threads.push(thread::Builder::new().name("fb-debounce".into()).spawn(move || {
                let mut pending: HashSet<PathBuf> = HashSet::new();
                let mut quiet_deadline: Option<Instant> = None;
                let mut window_start: Option<Instant> = None;
                loop {
                    let timeout = match (quiet_deadline, window_start) {
                        (Some(q), Some(w)) => {
                            let cap = w + Duration::from_secs(2);
                            q.min(cap).saturating_duration_since(Instant::now())
                        }
                        (Some(q), None) => q.saturating_duration_since(Instant::now()),
                        (None, _) => Duration::from_secs(3600),
                    };
                    match event_rx.recv_timeout(timeout) {
                        Ok(root) => {
                            pending.insert(root);
                            let now = Instant::now();
                            quiet_deadline = Some(now + debounce);
                            window_start.get_or_insert(now);
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            for root in pending.drain() {
                                let _ = work_tx.send(root);
                            }
                            quiet_deadline = None;
                            window_start = None;
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            for root in pending.drain() {
                                let _ = work_tx.send(root);
                            }
                            break;
                        }
                    }
                }
            })?);
        }

        // Worker thread: serial scan execution; no root is scanned twice
        // concurrently, and events landing mid-scan produce a follow-up scan.
        {
            let cancel = cancel.clone();
            let scan_count = scan_count.clone();
            let on_change = on_change;
            threads.push(thread::Builder::new().name("fb-scan".into()).spawn(move || {
                while let Ok(first) = work_rx.recv() {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let mut batch = HashSet::new();
                    batch.insert(first);
                    while let Ok(more) = work_rx.try_recv() {
                        batch.insert(more);
                    }
                    for root in batch {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let report = {
                            let mut cat = catalog.lock().unwrap();
                            scan_folder(&mut cat, &root, &cancel)
                        };
                        scan_count.fetch_add(1, Ordering::Relaxed);
                        match report {
                            Ok(r) => {
                                tracing::debug!(root = %root.display(), added = r.added,
                                    updated = r.updated, removed = r.removed, "watched rescan");
                                // Real change (or a fresh folder ini offer): wake
                                // the UI — and only then, no-op rescans must not
                                // trigger re-lists.
                                if r.changed() || !r.folder_ini_offered.is_empty() {
                                    if let Some(hook) = &on_change {
                                        hook(&r);
                                    }
                                }
                            }
                            Err(e) => tracing::warn!(root = %root.display(), error = %e,
                                "watched rescan failed"),
                        }
                    }
                }
            })?);
        }

        Ok(Self {
            scan_count,
            cancel,
            cmd_tx: Some(cmd_tx),
            event_tx: Some(event_tx),
            work_tx: Some(work_tx),
            threads,
        })
    }

    /// Number of incremental scans started since spawn.
    pub fn scan_count(&self) -> usize {
        self.scan_count.load(Ordering::Relaxed)
    }

    /// Start watching another folder (Folder Manager "Scan Always").
    ///
    /// Blocks until the root is armed and its closing incremental scan is
    /// enqueued, so the caller can rely on the root being watched on return.
    pub fn add_root(&self, root: &Path) -> Result<(), WatcherError> {
        // Canonicalize here so failures surface to the caller.
        fs::canonicalize(root)?;
        let (reply_tx, reply_rx) = mpsc::channel();
        self.cmd_tx
            .as_ref()
            .expect("watcher not stopped")
            .send(Cmd::Add(root.to_path_buf(), reply_tx))
            .map_err(|_| WatcherError::Stopped)?;
        // If the watch thread is gone the ack never arrives; fail instead of
        // hanging.
        reply_rx.recv().map_err(|_| WatcherError::Stopped)?;
        Ok(())
    }

    /// Stop watching a folder (Folder Manager unchecks "Scan Always").
    ///
    /// Blocks until the root's watcher is dropped: after this returns, no
    /// further events for the root can reach the scan pipeline.
    pub fn remove_root(&self, root: &Path) {
        let (reply_tx, reply_rx) = mpsc::channel();
        if self
            .cmd_tx
            .as_ref()
            .expect("watcher not stopped")
            .send(Cmd::Remove(root.to_path_buf(), reply_tx))
            .is_ok()
        {
            // Best effort: if the watch thread is already gone, the watcher
            // died with it and there is nothing left to wait for.
            let _ = reply_rx.recv();
        }
    }

    /// Shut down and wait for all watcher threads to exit.
    pub fn stop(mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.drop_senders();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }

    fn drop_senders(&mut self) {
        self.cmd_tx.take();
        self.event_tx.take();
        self.work_tx.take();
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // Dropping the senders unwinds the thread chain (each thread owns a
        // clone; closing the channels lets every thread exit). We do not join
        // here to keep Drop non-blocking.
        self.drop_senders();
    }
}
