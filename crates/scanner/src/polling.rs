//! Polling watcher for network filesystems (TASK-02-008, PLAN §6 P1.5).
//!
//! `notify` backends (inotify/FSEvents) never fire over SMB/NFS mounts, so
//! network-classified roots are served by this poller instead: a background
//! thread snapshots the directory listing (path + mtime + size — no
//! hashing; content hashes stay lazy per 02-009 / PLAN §10) and emits one
//! event into the watcher's existing event channel whenever the snapshot
//! changes. Downstream (debounce → serial scan worker) is untouched.
//!
//! The poll interval adapts to the share: slow probes double it, fast probes
//! halve it, and a cycle that saw changes tightens it — all clamped to the
//! configured [min, max] bounds. See [`AdaptiveInterval`].
//!
//! Classification happens once, at watch-registration time, via a
//! mount-filesystem check ([`is_network_path`]); a real SMB integration test
//! is gated behind `FB_SMB_TEST_ROOT` (harness: TASK-02-010).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

/// Default poll bounds ("e.g. 5s–5min" per the task spec).
pub const DEFAULT_POLL_MIN: Duration = Duration::from_secs(5);
pub const DEFAULT_POLL_MAX: Duration = Duration::from_secs(300);

/// Probe latency above which the interval backs off (doubling).
pub const DEFAULT_SLOW_THRESHOLD: Duration = Duration::from_secs(2);
/// Probe latency below which the interval tightens (halving).
pub const DEFAULT_FAST_THRESHOLD: Duration = Duration::from_millis(500);

/// Tunables for the polling watcher and its adaptive interval.
#[derive(Debug, Clone)]
pub struct PollConfig {
    /// Lower bound of the adaptive interval.
    pub min: Duration,
    /// Upper bound of the adaptive interval.
    pub max: Duration,
    /// Probes slower than this double the interval.
    pub slow_threshold: Duration,
    /// Probes faster than this halve the interval.
    pub fast_threshold: Duration,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            min: DEFAULT_POLL_MIN,
            max: DEFAULT_POLL_MAX,
            slow_threshold: DEFAULT_SLOW_THRESHOLD,
            fast_threshold: DEFAULT_FAST_THRESHOLD,
        }
    }
}

/// Adaptive poll interval: starts at `min`, doubles on slow probes, halves
/// on fast probes, tightens on change activity, and never leaves
/// `[min, max]`. One [`step`](Self::step) per poll cycle.
#[derive(Debug)]
pub struct AdaptiveInterval {
    config: PollConfig,
    current: Duration,
}

impl AdaptiveInterval {
    pub fn new(config: PollConfig) -> Self {
        let current = config.min;
        Self { config, current }
    }

    /// Current interval.
    pub fn interval(&self) -> Duration {
        self.current
    }

    /// Feed one poll cycle's outcome: measured probe latency and whether the
    /// cycle observed any changes. Returns the next interval.
    pub fn step(&mut self, probe_latency: Duration, saw_changes: bool) -> Duration {
        // Change activity tightens first: a share with churn is polled more
        // aggressively than an idle one, within the same bounds.
        if saw_changes || probe_latency < self.config.fast_threshold {
            self.halve();
        } else if probe_latency > self.config.slow_threshold {
            self.double();
        }
        self.current
    }

    fn double(&mut self) {
        self.current = (self.current * 2).min(self.config.max);
    }

    fn halve(&mut self) {
        self.current = (self.current / 2).max(self.config.min);
    }
}

/// Snapshot entry: the poll key is (mtime, size) — cheap, no hashing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    mtime_secs: i64,
    mtime_nanos: u32,
    size: u64,
}

fn snapshot(dir: &Path) -> std::io::Result<HashMap<PathBuf, Entry>> {
    let mut out = HashMap::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            // Recursive: shares hold the same folder layout as local roots,
            // and the scan pipeline expects the full tree.
            out.extend(snapshot(&path)?);
        } else {
            let meta = entry.metadata()?;
            let (secs, nanos) = match meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
            {
                Some(d) => (d.as_secs() as i64, d.subsec_nanos()),
                None => (0, 0),
            };
            out.insert(path, Entry { mtime_secs: secs, mtime_nanos: nanos, size: meta.len() });
        }
    }
    Ok(out)
}

/// A running poller for one root. Dropping or calling [`Poller::stop`]
/// shuts the poll thread down.
pub struct Poller {
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    poll_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    thread: Option<JoinHandle<()>>,
}

impl Poller {
    /// Spawn a poller that sends the root path into `event_tx` whenever the
    /// snapshot changes (the same contract the notify watcher fulfills for
    /// local roots). Backend selection is the caller's ([`is_network_path`]
    /// at watch-registration time); the poller always polls.
    pub fn spawn(root: PathBuf, event_tx: Sender<PathBuf>, config: PollConfig) -> Self {
        Self::spawn_with_classification(root, event_tx, config, |_| true)
    }

    /// [`Poller::spawn`] with an explicit classification override, for
    /// testing the backend selection without a real network mount.
    pub fn spawn_with_classification(
        root: PathBuf,
        event_tx: Sender<PathBuf>,
        config: PollConfig,
        classify: impl Fn(&Path) -> bool + Send + 'static,
    ) -> Self {
        // (the classification override lets a test exercise the "network
        // root, poll backend" path on a local filesystem)
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let poll_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let stop_flag = stop.clone();
        let count = poll_count.clone();
        let thread = thread::Builder::new()
            .name("fb-poll".into())
            .spawn(move || {
                if !classify(&root) {
                    return; // not our backend; the caller picks the right one
                }
                let mut interval = AdaptiveInterval::new(config.clone());
                let mut last: Option<HashMap<PathBuf, Entry>> = None;
                loop {
                    let started = std::time::Instant::now();
                    let snap = snapshot(&root);
                    let latency = started.elapsed();
                    count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    match snap {
                        Ok(snap) => {
                            let changed = last.as_ref().map(|prev| *prev != snap).unwrap_or(true);
                            // First cycle establishes the baseline without
                            // emitting (the notify watcher's Add path does
                            // its own closing scan; here the same event
                            // channel is fed only on real deltas).
                            if last.is_some() && changed {
                                let _ = event_tx.send(root.clone());
                            }
                            interval.step(latency, changed && last.is_some());
                            last = Some(snap);
                        }
                        Err(e) => {
                            // Vanished root (share disconnected): back off
                            // and keep polling; reconnect is picked up on a
                            // later cycle (02-009 owns pause/resume policy).
                            tracing::debug!(root = %root.display(), error = %e, "poll probe failed");
                            interval.step(Duration::from_secs(10), false);
                        }
                    }
                    let sleep = interval.interval();
                    let deadline = std::time::Instant::now() + sleep;
                    while std::time::Instant::now() < deadline {
                        if stop.load(std::sync::atomic::Ordering::Relaxed) {
                            return;
                        }
                        thread::sleep(Duration::from_millis(20).min(deadline.saturating_duration_since(std::time::Instant::now())));
                    }
                }
            })
            .expect("failed to spawn poll thread");
        Self { stop_flag, poll_count, thread: Some(thread) }
    }

    /// Number of completed poll cycles (tests use it to wait for the
    /// baseline snapshot before making changes).
    pub fn poll_count(&self) -> usize {
        self.poll_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Stop the poller and wait for its thread to exit.
    pub fn stop(mut self) {
        self.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Poller {
    fn drop(&mut self) {
        self.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        // Non-blocking drop, like the notify watcher: the thread notices the
        // flag within one poll sleep and exits on its own.
    }
}

/// Is `path` on a network filesystem (SMB/NFS/CIFS/AFP)? Decided at
/// watch-registration time, per the task spec. Unknown platforms classify as
/// local (the notify backend is the safe default there).
pub fn is_network_path(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        is_network_path_macos(path)
    }
    #[cfg(target_os = "linux")]
    {
        is_network_path_linux(path)
    }
    #[cfg(target_os = "windows")]
    {
        is_network_path_windows(path)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = path;
        false
    }
}

#[cfg(target_os = "macos")]
fn is_network_path_macos(path: &Path) -> bool {
    // nix::sys::statfs wraps statfs(2); match on the filesystem type name —
    // robust and covers smbfs, nfs, afpfs, cifs, webdav.
    use nix::sys::statfs::statfs;
    match statfs(path) {
        Ok(fs) => {
            let name = fs.filesystem_type_name().to_string();
            matches!(name.as_str(), "nfs" | "smbfs" | "smb" | "cifs" | "afpfs" | "webdav")
        }
        Err(_) => false,
    }
}
#[cfg(target_os = "linux")]
fn is_network_path_linux(path: &Path) -> bool {
    // nix::sys::statfs exposes the kernel's f_type magic numbers
    // (linux/magic.h) — a single syscall, no /proc parsing.
    use nix::sys::statfs::{
        statfs, AFS_SUPER_MAGIC, CODA_SUPER_MAGIC, NCP_SUPER_MAGIC, NFS_SUPER_MAGIC,
        SMB_SUPER_MAGIC,
    };
    const CIFS_MAGIC_NUMBER: nix::sys::statfs::FsType = nix::sys::statfs::FsType(0xFF534D42); // CIFS
    const SMB2_MAGIC_NUMBER: nix::sys::statfs::FsType = nix::sys::statfs::FsType(0xFE534D42); // SMB2/3
    match statfs(path) {
        Ok(fs) => {
            let t = fs.filesystem_type();
            matches!(
                t,
                NFS_SUPER_MAGIC
                    | CIFS_MAGIC_NUMBER
                    | SMB_SUPER_MAGIC
                    | SMB2_MAGIC_NUMBER
                    | AFS_SUPER_MAGIC
                    | CODA_SUPER_MAGIC
                    | NCP_SUPER_MAGIC
            )
        }
        Err(_) => false,
    }
}

#[cfg(target_os = "windows")]
fn is_network_path_windows(path: &Path) -> bool {
    // UNC paths (\\server\share or \\?\UNC\...) are network by definition;
    // mapped drives would need WNetGetConnection (extra FFI) — UNC detection
    // covers the common NAS case and keeps this std-only.
    let s = path.as_os_str().to_string_lossy();
    s.starts_with("\\\\") && !s.starts_with("\\\\?\\UNC\\") || s.starts_with("\\\\?\\UNC\\")
}
