//! Perf-harness hooks (TASK-01-026, `docs/webkitgtk-perf-protocol.md`).
//!
//! Inert unless the perf harness arms the app through the environment —
//! normal users never enter this path and pay nothing for it. When armed
//! (`FOTOBASE_PERF_FIXTURE` + `FOTOBASE_PERF_COUNT`), the app adds the
//! fixture directory as a watched folder at startup (cold start: empty
//! catalog + empty thumbnail cache, protocol §1) and exposes one IPC
//! command, `perf_probe`, from which the WebDriver harness reads the
//! protocol §2 timestamps:
//!
//! - `scan_started_ms` — set the moment the fixture folder is handed to the
//!   watcher (the harness's t0 for every derived metric).
//! - `scan_completed_ms` — set by a poller thread once the catalog holds the
//!   expected file count (the scan itself runs on the watcher thread).
//! - `first_thumb_serve_ms` — set by the `thumb://` scheme handler the first
//!   time it serves *real* cached bytes (a cache hit; misses serve the
//!   placeholder). This is the protocol's time-to-first-thumbnail endpoint:
//!   the serve is the last app-controlled step before the pixels paint.
//!
//! No image bytes, no sidecars, no derived data written anywhere — this
//! module only records wall-clock stamps.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Environment variable naming the fixture directory to add as a watched
/// folder at startup (harness-only; unset in every normal launch).
pub const ENV_FIXTURE: &str = "FOTOBASE_PERF_FIXTURE";
/// Environment variable naming the file count the fixture contains; the
/// completion poller waits until the catalog reaches it.
pub const ENV_COUNT: &str = "FOTOBASE_PERF_COUNT";

/// What the harness arms the app with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmConfig {
    pub fixture: PathBuf,
    pub expected: usize,
}

/// Protocol §2 timestamps, read by the harness through `perf_probe`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Snapshot {
    pub scan_started_ms: u64,
    /// `None` until the catalog holds `expected` files.
    pub scan_completed_ms: Option<u64>,
    /// First *real* thumbnail (cache hit) served by the `thumb://` handler.
    pub first_thumb_serve_ms: Option<u64>,
    pub expected: usize,
}

static STARTED: Mutex<Option<(u64, usize)>> = Mutex::new(None);
static COMPLETED: AtomicU64 = AtomicU64::new(0);
static FIRST_THUMB: AtomicU64 = AtomicU64::new(0);

fn epoch_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("clock before epoch").as_millis() as u64
}

/// Read the arm configuration from the environment; `None` (the normal
/// launch) leaves every hook inert.
pub fn arm_from_env() -> Option<ArmConfig> {
    let fixture = std::env::var(ENV_FIXTURE).ok()?;
    let expected: usize = std::env::var(ENV_COUNT).ok()?.parse().ok()?;
    if expected == 0 || fixture.is_empty() {
        return None;
    }
    Some(ArmConfig { fixture: PathBuf::from(fixture), expected })
}

/// Mark scan start (called by `setup` right before the fixture folder is
/// handed to the watcher). Idempotent: the first call wins.
pub fn begin(expected: usize) {
    let mut guard = STARTED.lock().expect("perf probe mutex poisoned");
    if guard.is_none() {
        *guard = Some((epoch_ms(), expected));
    }
}

/// Mark scan completion (called by the poller thread). First call wins.
pub fn note_scan_complete() {
    COMPLETED.compare_exchange(0, epoch_ms(), Ordering::SeqCst, Ordering::SeqCst).ok();
}

/// Record the first real-thumbnail serve (called by the `thumb://` handler
/// on a cache hit). First call wins.
pub fn note_first_thumb() {
    FIRST_THUMB.compare_exchange(0, epoch_ms(), Ordering::SeqCst, Ordering::SeqCst).ok();
}

/// Current §2 timestamps; `None` while the hook is not armed.
pub fn snapshot() -> Option<Snapshot> {
    let guard = STARTED.lock().expect("perf probe mutex poisoned");
    let (started, expected) = guard.as_ref()?; // Copy out; drop the lock before returning.
    Some(Snapshot {
        scan_started_ms: *started,
        scan_completed_ms: match COMPLETED.load(Ordering::SeqCst) {
            0 => None,
            ms => Some(ms),
        },
        first_thumb_serve_ms: match FIRST_THUMB.load(Ordering::SeqCst) {
            0 => None,
            ms => Some(ms),
        },
        expected: *expected,
    })
}

/// Poll the catalog on a private connection until it holds `expected` files,
/// then mark scan completion. The scan itself runs on the watcher thread;
/// WAL lets this read concurrently (PLAN §3 rule 2: the catalog is a cache —
/// the harness only ever reads it). Gives up after 30 minutes: the harness
/// has its own timeout and reports the miss either way.
pub fn spawn_completion_watcher(catalog_path: PathBuf, expected: usize) {
    std::thread::spawn(move || {
        let Ok(cat) = fotobase_catalog::Catalog::open(&catalog_path) else { return };
        let deadline = Instant::now() + Duration::from_secs(30 * 60);
        while Instant::now() < deadline {
            // A transient read error just means "not done yet" — the poller
            // retries; the harness's own timeout is the backstop.
            if cat.list_files(expected).map(|files| files.len() >= expected).unwrap_or(false) {
                note_scan_complete();
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // The stamps live in process-global statics, so the whole lifecycle is
    // exercised as one ordered scenario (tests in this module share them).
    #[test]
    fn lifecycle_starts_inert_and_fills_in_later() {
        // Inert until armed: snapshot is None, and stray first-thumb notes
        // from other tests' app instances must not conjure a snapshot.
        assert!(snapshot().is_none());

        begin(1000);
        let snap = snapshot().expect("armed");
        assert_eq!(snap.expected, 1000);
        assert_eq!(snap.scan_completed_ms, None);
        assert_eq!(snap.first_thumb_serve_ms, None);
        // Sanity: the stamp is a plausible epoch-ms value (after 2020-01-01).
        assert!(snap.scan_started_ms > 1_577_836_800_000);

        note_first_thumb();
        let snap = snapshot().expect("armed");
        assert!(snap.first_thumb_serve_ms.is_some());

        note_scan_complete();
        let snap = snapshot().expect("armed");
        assert!(snap.scan_completed_ms.is_some());
        assert!(snap.scan_completed_ms.unwrap() >= snap.scan_started_ms);
        assert!(snap.first_thumb_serve_ms.unwrap() >= snap.scan_started_ms);
    }

    #[test]
    fn arm_from_env_rejects_malformed_input() {
        // Not armed in unit tests; a missing variable yields None.
        let saved_fixture = std::env::var(ENV_FIXTURE).ok();
        let saved_count = std::env::var(ENV_COUNT).ok();
        std::env::remove_var(ENV_FIXTURE);
        std::env::remove_var(ENV_COUNT);
        assert!(arm_from_env().is_none());

        std::env::set_var(ENV_FIXTURE, "/tmp/fixture");
        std::env::set_var(ENV_COUNT, "not-a-number");
        assert!(arm_from_env().is_none());

        std::env::set_var(ENV_COUNT, "0");
        assert!(arm_from_env().is_none());

        std::env::set_var(ENV_COUNT, "1000");
        assert_eq!(
            arm_from_env(),
            Some(ArmConfig { fixture: PathBuf::from("/tmp/fixture"), expected: 1000 })
        );

        match saved_fixture {
            Some(v) => std::env::set_var(ENV_FIXTURE, v),
            None => std::env::remove_var(ENV_FIXTURE),
        }
        match saved_count {
            Some(v) => std::env::set_var(ENV_COUNT, v),
            None => std::env::remove_var(ENV_COUNT),
        }
    }
}
