//! Polling watcher tests (TASK-02-008, TDD).
//!
//! `notify` never fires over SMB/NFS, so network roots get a polling watcher:
//! it snapshots directory listings (path + mtime + size — no hashing, that
//! stays lazy per 02-009) and emits at most one change event per cycle into
//! the same event channel the notify watcher feeds. The interval adapts to
//! measured probe latency within configured bounds.

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use fotobase_scanner::{is_network_path, AdaptiveInterval, PollConfig, Poller};

const FAST: Duration = Duration::from_millis(10);
const SLOW: Duration = Duration::from_millis(300);

fn test_config() -> PollConfig {
    PollConfig {
        min: Duration::from_millis(40),
        max: Duration::from_secs(2),
        slow_threshold: SLOW,
        fast_threshold: FAST,
    }
}

fn write_jpg(path: &Path, len: usize) {
    std::fs::write(path, vec![0xff; len]).unwrap();
}

fn wait_for(mut cond: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    cond()
}

/// TDD criterion: one changed mtime ⇒ exactly one change event (re-emission
/// stops once the snapshot matches again).
#[test]
fn changed_mtime_emits_exactly_one_event() {
    let lib = tempfile::tempdir().unwrap();
    write_jpg(&lib.path().join("a.jpg"), 10);

    let (tx, rx) = mpsc::channel();
    let poller = Poller::spawn(lib.path().to_path_buf(), tx, test_config());

    // Baseline settles: no event for an unchanged folder.
    assert!(
        !wait_for(|| rx.try_recv().is_ok(), Duration::from_millis(300)),
        "unchanged folder must not emit events"
    );
    wait_for(|| poller.poll_count() >= 1, Duration::from_secs(5));

    // Touch the mtime without changing size (mtime+size is the poll key).
    let file = lib.path().join("a.jpg");
    let new = std::time::SystemTime::now() + Duration::from_secs(3600);
    filetime::set_file_mtime(&file, filetime::FileTime::from_system_time(new)).unwrap();

    assert!(
        wait_for(|| rx.try_recv().is_ok(), Duration::from_secs(5)),
        "poller never emitted the change"
    );
    // Snapshot now matches again: no further events for the same change.
    std::thread::sleep(Duration::from_millis(500));
    assert!(rx.try_recv().is_err(), "one change must emit exactly one event");
    poller.stop();
}

/// TDD criterion: interval doubles when probes exceed the latency threshold,
/// halves back when probes are fast, and never leaves [min, max].
#[test]
fn interval_adapts_to_probe_latency() {
    let cfg = PollConfig {
        min: Duration::from_secs(5),
        max: Duration::from_secs(300),
        slow_threshold: Duration::from_millis(500),
        fast_threshold: Duration::from_millis(100),
    };
    let mut iv = AdaptiveInterval::new(cfg.clone());
    assert_eq!(iv.interval(), Duration::from_secs(5), "starts at min");

    // Slow probe: double, and keep doubling while slow.
    iv.step(Duration::from_millis(700), false);
    assert_eq!(iv.interval(), Duration::from_secs(10), "slow probe doubles");
    iv.step(Duration::from_millis(700), false);
    assert_eq!(iv.interval(), Duration::from_secs(20));

    // Clamped at max even after many slow probes.
    for _ in 0..10 {
        iv.step(Duration::from_secs(2), false);
    }
    assert_eq!(iv.interval(), cfg.max, "interval clamps at max");

    // Fast probe: halve back toward min.
    iv.step(Duration::from_millis(50), false);
    assert_eq!(iv.interval(), Duration::from_secs(150), "fast probe halves");
    for _ in 0..10 {
        iv.step(Duration::from_millis(50), false);
    }
    assert_eq!(iv.interval(), cfg.min, "interval returns to min");
}

/// Change activity tightens the interval (shares with churn are polled more
/// aggressively than idle ones, within the same bounds).
#[test]
fn change_activity_tightens_interval() {
    let cfg = PollConfig {
        min: Duration::from_secs(5),
        max: Duration::from_secs(300),
        slow_threshold: Duration::from_millis(500),
        fast_threshold: Duration::from_millis(100),
    };
    let mut iv = AdaptiveInterval::new(cfg);
    // Back off with slow probes first.
    for _ in 0..2 {
        iv.step(Duration::from_millis(700), false);
    }
    let backed_off = iv.interval();
    assert!(backed_off > Duration::from_secs(5));

    // A cycle that saw changes tightens the interval even if the probe was
    // mid-range latency.
    iv.step(Duration::from_millis(300), true);
    assert!(iv.interval() < backed_off, "activity tightens the interval");
}

/// Selection criterion: local (tmpfs/ext4) paths must not be classified as
/// network; the SMB/NFS magic-number match must not leak onto local mounts.
#[test]
fn local_paths_are_not_network() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!is_network_path(tmp.path()), "tmpdir is a local filesystem");
}

/// Windows regression: `fs::canonicalize` returns extended-length paths
/// (`\\?\C:\...`); those are local drives and must not match the `\\`
/// UNC prefix check — CI (run 40353505615) classified every canonicalized
/// root as network, so local roots got the poller and `notify` tests starved.
#[cfg(windows)]
#[test]
fn windows_canonicalized_local_paths_are_not_network() {
    let tmp = tempfile::tempdir().unwrap();
    let canon = std::fs::canonicalize(tmp.path()).unwrap();
    assert!(!is_network_path(&canon), "\\\\?\\C:\\... is a local drive");
    assert!(!is_network_path(tmp.path()));
    assert!(is_network_path(std::path::Path::new("\\\\server\\share")));
    assert!(is_network_path(std::path::Path::new("\\\\?\\UNC\\server\\share")));
}

/// Selection criterion: a network-classified root is served by the polling
/// backend, not `notify`. Driven through the watcher's backend decision with
/// a forced network classification (a real SMB mount is not available in
/// unit CI — the integration harness is TASK-02-010, see the env-gated test
/// below).
#[test]
fn network_classified_root_uses_polling_backend() {
    let lib = tempfile::tempdir().unwrap();
    write_jpg(&lib.path().join("a.jpg"), 10);
    let (tx, rx) = mpsc::channel();

    let poller = Poller::spawn_with_classification(
        lib.path().to_path_buf(),
        tx,
        test_config(),
        |_p| true, // force the network classification
    );
    wait_for(|| poller.poll_count() >= 1, Duration::from_secs(5));

    // A change must still surface through the poll backend.
    write_jpg(&lib.path().join("new.jpg"), 42);
    assert!(
        wait_for(|| rx.try_recv().is_ok(), Duration::from_secs(5)),
        "polling backend never emitted the added file"
    );
    poller.stop();
}

/// Integration: an SMB mount sees an added file appear within one poll
/// cycle. Requires a mounted share; run with `FB_SMB_TEST_ROOT=/mnt/share`.
/// The CI harness for this lives in TASK-02-010.
#[test]
fn smb_mount_added_file_appears_within_one_poll_cycle() {
    let Some(root) = std::env::var_os("FB_SMB_TEST_ROOT").map(std::path::PathBuf::from) else {
        eprintln!("skipping: set FB_SMB_TEST_ROOT to a mounted SMB share");
        return;
    };
    let (tx, rx) = mpsc::channel();
    let poller = Poller::spawn(root.clone(), tx, PollConfig::default());
    write_jpg(&root.join("from-test.jpg"), 42);
    assert!(
        wait_for(|| rx.try_recv().is_ok(), Duration::from_secs(30)),
        "added file on the share was not picked up within one poll cycle"
    );
    poller.stop();
}
