//! Throttled network scanning tests (TASK-02-009, TDD).
//!
//! Network scans differ from local ones (PLAN §6 P1.5): IO is capped so
//! browsing stays responsive, a mid-scan disconnect pauses the scan cleanly
//! (errors aggregated, nothing deleted), and resume continues from the last
//! completed directory. Hashing stays lazy — a network scan never fills
//! `files.hash` (PLAN §10).
//!
//! Disconnects are simulated with a fault-injecting [`FsOps`] so the tests
//! run on a plain local filesystem; the real-SMB harness is TASK-02-010.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fotobase_catalog::Catalog;
use fotobase_scanner::{
    scan_network_folder, EntryMeta, FsOps, NetworkScanOptions, NetworkScanState, RealFs,
};

fn write_jpg(path: &Path, len: usize) {
    std::fs::write(path, vec![0xff; len]).unwrap();
}

/// Tree: root with `dirs` subdirectories, each holding `files_per_dir` jpgs.
fn make_tree(root: &Path, dirs: usize, files_per_dir: usize) {
    for d in 0..dirs {
        let dir = root.join(format!("dir{d:02}"));
        std::fs::create_dir_all(&dir).unwrap();
        for f in 0..files_per_dir {
            write_jpg(&dir.join(format!("img{f:03}.jpg")), 16);
        }
    }
}

fn catalog_at(dir: &Path) -> Catalog {
    Catalog::open(dir.join("catalog.db")).unwrap()
}

/// Fault-injecting filesystem: can make `read_dir` fail for a specific
/// directory and make the liveness probe report the share as down.
struct FaultFs {
    inner: RealFs,
    fail_dir: Mutex<Option<PathBuf>>,
    fail_reads: AtomicUsize,
    probe_down: AtomicBool,
    probe_calls: AtomicUsize,
}

impl FaultFs {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: RealFs,
            fail_dir: Mutex::new(None),
            fail_reads: AtomicUsize::new(0),
            probe_down: AtomicBool::new(false),
            probe_calls: AtomicUsize::new(0),
        })
    }

    fn fail_next_reads_of(&self, dir: &Path, n: usize) {
        *self.fail_dir.lock().unwrap() = Some(dir.to_path_buf());
        self.fail_reads.store(n, Ordering::SeqCst);
    }

    fn share_down(&self, down: bool) {
        self.probe_down.store(down, Ordering::SeqCst);
    }
}

impl FsOps for FaultFs {
    fn read_dir(&self, dir: &Path) -> io::Result<Vec<EntryMeta>> {
        let failing = {
            let guard = self.fail_dir.lock().unwrap();
            matches!(guard.as_ref(), Some(d) if d == dir)
        };
        if failing && self.fail_reads.load(Ordering::SeqCst) > 0 {
            self.fail_reads.fetch_sub(1, Ordering::SeqCst);
            if self.fail_reads.load(Ordering::SeqCst) == 0 {
                *self.fail_dir.lock().unwrap() = None;
            }
            return Err(io::Error::new(io::ErrorKind::TimedOut, "share unreachable"));
        }
        self.inner.read_dir(dir)
    }

    fn probe(&self, root: &Path) -> bool {
        self.probe_calls.fetch_add(1, Ordering::SeqCst);
        if self.probe_down.load(Ordering::SeqCst) {
            return false;
        }
        self.inner.probe(root)
    }
}

fn opts(fs: Arc<FaultFs>) -> NetworkScanOptions {
    NetworkScanOptions {
        fs,
        // Fast probes so the pause happens "within seconds".
        probe_interval: Duration::from_millis(10),
        ..NetworkScanOptions::default()
    }
}

/// TDD criterion: hashing is lazy for network folders — a scan never fills
/// `files.hash` (spy: the catalog's hash column stays NULL for every file).
#[test]
fn network_scan_never_hashes() {
    let tmp = tempfile::tempdir().unwrap();
    make_tree(tmp.path(), 2, 3);
    let mut catalog = catalog_at(tmp.path());
    let mut options = opts(FaultFs::new());
    options.ops_per_sec = 10_000; // no artificial delay in this test

    let report =
        scan_network_folder(&mut catalog, tmp.path(), &AtomicBool::new(false), &options, &|_| {})
            .unwrap();
    assert_eq!(report.state, NetworkScanState::Completed);
    assert_eq!(report.added, 6);

    // Every file is present but unhashed: the backfill pass (PLAN §10) is
    // the only thing that fills hashes, never the scan.
    let unhashed = catalog.unhashed_files(100).unwrap();
    assert_eq!(unhashed.len(), 6);
}

/// TDD criterion: IO throttling — with a small ops/sec budget the scan paces
/// itself instead of hammering the share (browsing stays responsive).
#[test]
fn io_budget_paces_the_scan() {
    let tmp = tempfile::tempdir().unwrap();
    make_tree(tmp.path(), 3, 25); // 4 read_dirs + 75 file ops > the budget
    let mut catalog = catalog_at(tmp.path());
    let mut options = opts(FaultFs::new());
    options.ops_per_sec = 60; // ~79 ops ⇒ at least ~0.3s of pacing

    let start = Instant::now();
    let report =
        scan_network_folder(&mut catalog, tmp.path(), &AtomicBool::new(false), &options, &|_| {})
            .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(report.state, NetworkScanState::Completed);
    assert_eq!(report.added, 75);
    assert!(
        elapsed >= Duration::from_millis(280),
        "scan of ~79 ops at 60 ops/sec must be paced, took {elapsed:?}"
    );
}

/// TDD criterion: killing the share mid-scan pauses the scan cleanly —
/// state is `paused` within seconds, errors are aggregated (one count, not
/// a per-file storm), and nothing is deleted from the catalog.
#[test]
fn mid_scan_disconnect_pauses_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    make_tree(tmp.path(), 4, 4);
    let mut catalog = catalog_at(tmp.path());

    let fs = FaultFs::new();
    let mut options = opts(fs.clone());
    options.ops_per_sec = 10_000;
    // dir02 fails its first read_dir, and the share is down at that moment.
    fs.fail_next_reads_of(&tmp.path().join("dir02"), 1);
    fs.share_down(true);

    let report =
        scan_network_folder(&mut catalog, tmp.path(), &AtomicBool::new(false), &options, &|_| {})
            .unwrap();

    assert_eq!(report.state, NetworkScanState::Paused);
    assert!(report.errors > 0, "disconnect must be aggregated as errors");
    // Completed directories are recorded so resume can continue from there.
    assert!(
        report.completed_dirs.iter().any(|d| d.ends_with("dir00")),
        "dirs completed before the disconnect are recorded: {:?}",
        report.completed_dirs
    );
    assert!(
        !report.completed_dirs.iter().any(|d| d.ends_with("dir02")),
        "the failing dir must not be marked completed"
    );
    // Nothing was deleted: paused scans are non-destructive.
    assert_eq!(report.removed, 0);
    // dir00's files are already in the catalog.
    assert!(catalog
        .file_by_path(tmp.path().join("dir00").join("img000.jpg").to_string_lossy().as_ref())
        .unwrap()
        .is_some());
}

/// TDD criterion: reconnect resumes the scan from the last completed
/// directory with no duplicated work — the resumed run adds only the files
/// the paused run never reached, and the final catalog matches a
/// never-disconnected control run.
#[test]
fn reconnect_resumes_without_duplicated_work() {
    let tmp = tempfile::tempdir().unwrap();
    make_tree(tmp.path(), 4, 4);
    let mut catalog = catalog_at(tmp.path());

    let fs = FaultFs::new();
    let mut options = opts(fs.clone());
    options.ops_per_sec = 10_000;
    fs.fail_next_reads_of(&tmp.path().join("dir02"), 1);
    fs.share_down(true);

    let paused =
        scan_network_folder(&mut catalog, tmp.path(), &AtomicBool::new(false), &options, &|_| {})
            .unwrap();
    assert_eq!(paused.state, NetworkScanState::Paused);
    let paused_added = paused.added;

    // Reconnect: probe healthy again, no more faults.
    fs.share_down(false);
    options.completed_dirs = paused.completed_dirs.clone();

    let resumed =
        scan_network_folder(&mut catalog, tmp.path(), &AtomicBool::new(false), &options, &|_| {})
            .unwrap();
    assert_eq!(resumed.state, NetworkScanState::Completed);

    // Control run: same tree, fresh catalog, no faults.
    let control_dir = tempfile::tempdir().unwrap();
    make_tree(control_dir.path(), 4, 4);
    let mut control = catalog_at(control_dir.path());
    let mut control_opts = opts(FaultFs::new());
    control_opts.ops_per_sec = 10_000;
    let control_report = scan_network_folder(
        &mut control,
        control_dir.path(),
        &AtomicBool::new(false),
        &control_opts,
        &|_| {},
    )
    .unwrap();
    assert_eq!(control_report.state, NetworkScanState::Completed);
    assert_eq!(control_report.added, 16);

    // No duplicated work: paused + resumed adds == control added.
    assert_eq!(
        paused_added + resumed.added,
        control_report.added,
        "resume must not re-add files from already-completed dirs"
    );
    // Same final scanned set as the control run.
    assert_eq!(catalog.list_files(100).unwrap().len(), control.list_files(100).unwrap().len());

    // Resumed run did not re-walk completed dirs: none of their files were
    // counted again.
    assert!(
        resumed.completed_dirs.iter().any(|d| d.ends_with("dir00")),
        "completed dirs carry over into the resumed run: {:?}",
        resumed.completed_dirs
    );
}

/// TDD criterion: the cancel flag stops the scan cleanly (state `cancelled`),
/// like every other scanner entry point.
#[test]
fn cancel_flag_stops_network_scan() {
    let tmp = tempfile::tempdir().unwrap();
    make_tree(tmp.path(), 2, 2);
    let mut catalog = catalog_at(tmp.path());
    let mut options = opts(FaultFs::new());
    options.ops_per_sec = 10_000;

    let report =
        scan_network_folder(&mut catalog, tmp.path(), &AtomicBool::new(true), &options, &|_| {})
            .unwrap();
    assert_eq!(report.state, NetworkScanState::Cancelled);
}

/// Transient IO error with the share still alive must NOT pause the scan:
/// the probe passes, the op is retried, and the scan completes.
#[test]
fn transient_error_with_live_share_retries() {
    let tmp = tempfile::tempdir().unwrap();
    make_tree(tmp.path(), 2, 2);
    let mut catalog = catalog_at(tmp.path());

    let fs = FaultFs::new();
    let mut options = opts(fs.clone());
    options.ops_per_sec = 10_000;
    // dir01 fails once, but the share stays up.
    fs.fail_next_reads_of(&tmp.path().join("dir01"), 1);

    let report =
        scan_network_folder(&mut catalog, tmp.path(), &AtomicBool::new(false), &options, &|_| {})
            .unwrap();
    assert_eq!(report.state, NetworkScanState::Completed);
    assert_eq!(report.added, 4);
    assert!(fs.probe_calls.load(Ordering::SeqCst) >= 1, "a retry must probe liveness");
    assert_eq!(report.errors, 0, "a recovered transient error is not surfaced");
}
