//! Watcher integration tests (TASK-01-007, TDD).
//!
//! Debounced `notify` events for "Scan Always" folders must produce exactly
//! one incremental scan per quiet window, and no file may be scanned twice
//! concurrently. The catalog lives OUTSIDE the watched folder so the watcher
//! never sees catalog writes.

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fotobase_catalog::Catalog;
use fotobase_scanner::{scan_folder, Watcher};

const DEBOUNCE: Duration = Duration::from_millis(50);
const SETTLE: Duration = Duration::from_millis(250);

fn write_jpg(path: &Path, len: usize) {
    fs::write(path, vec![0xff; len]).unwrap();
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

struct Fixture {
    _lib: tempfile::TempDir,
    _db: tempfile::TempDir,
    catalog: Arc<Mutex<Catalog>>,
    handle: Watcher,
}

impl Fixture {
    fn scans(&self) -> usize {
        self.handle.scan_count()
    }
}

/// Seed files, scan them, then start a watcher on the folder.
fn fixture(seed: &[(&str, usize)]) -> Fixture {
    let lib = tempfile::tempdir().unwrap();
    for (name, len) in seed {
        write_jpg(&lib.path().join(name), *len);
    }
    let db = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(db.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, lib.path(), &AtomicBool::new(false)).unwrap();
    let catalog = Arc::new(Mutex::new(cat));
    let handle = Watcher::spawn(&[lib.path().to_path_buf()], catalog.clone(), DEBOUNCE).unwrap();
    Fixture { _lib: lib, _db: db, catalog, handle }
}

fn count_files(fx: &Fixture) -> i64 {
    fx.catalog
        .lock()
        .unwrap()
        .conn()
        .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
        .unwrap()
}

fn file_size(fx: &Fixture, filename: &str) -> Option<i64> {
    fx.catalog
        .lock()
        .unwrap()
        .conn()
        .query_row("SELECT size FROM files WHERE filename = ?1", [filename], |r| r.get(0))
        .ok()
}

fn row_count_for(fx: &Fixture, filename: &str) -> i64 {
    fx.catalog
        .lock()
        .unwrap()
        .conn()
        .query_row("SELECT COUNT(*) FROM files WHERE filename = ?1", [filename], |r| r.get(0))
        .unwrap()
}

/// TDD criterion: write then rename-in a file ⇒ exactly one catalog insert
/// after the debounce window (rename-in = editor atomic-save pattern; notify
/// fires several events for it).
#[test]
fn rename_in_file_inserts_exactly_one_row() {
    let fx = fixture(&[("a.jpg", 10)]);

    // Atomic save: write temp file, then rename over the final name.
    write_jpg(&fx._lib.path().join("new.jpg.tmp"), 42);
    fs::rename(fx._lib.path().join("new.jpg.tmp"), fx._lib.path().join("new.jpg")).unwrap();

    assert!(wait_for(|| fx.scans() >= 1, Duration::from_secs(5)), "watcher never triggered a scan");
    assert_eq!(row_count_for(&fx, "new.jpg"), 1, "exactly one insert");
    assert_eq!(file_size(&fx, "new.jpg"), Some(42));

    // After the quiet window settles, no extra scans and no duplicate rows.
    std::thread::sleep(SETTLE);
    assert_eq!(fx.scans(), 1, "debounce must coalesce the event burst");
    assert_eq!(row_count_for(&fx, "new.jpg"), 1);
    assert_eq!(count_files(&fx), 2);
}

/// TDD criterion: two rapid modifications coalesce into one rescan job.
#[test]
fn two_rapid_modifications_coalesce_into_one_rescan() {
    let fx = fixture(&[("b.png", 20)]);

    write_jpg(&fx._lib.path().join("b.png"), 999);
    write_jpg(&fx._lib.path().join("b.png"), 1000);

    assert!(wait_for(|| fx.scans() >= 1, Duration::from_secs(5)), "watcher never triggered a scan");
    std::thread::sleep(SETTLE);
    assert_eq!(fx.scans(), 1, "rapid edits must coalesce into one job");
    assert_eq!(file_size(&fx, "b.png"), Some(1000), "latest state wins");
    assert_eq!(count_files(&fx), 1);
}

/// Functional spec: create / modify / delete are all picked up live.
#[test]
fn create_modify_delete_are_picked_up() {
    let fx = fixture(&[("a.jpg", 10)]);

    // create
    write_jpg(&fx._lib.path().join("added.jpg"), 33);
    assert!(
        wait_for(|| row_count_for(&fx, "added.jpg") == 1, Duration::from_secs(5)),
        "created file must appear in the catalog"
    );

    // modify
    write_jpg(&fx._lib.path().join("a.jpg"), 77);
    assert!(
        wait_for(|| file_size(&fx, "a.jpg") == Some(77), Duration::from_secs(5)),
        "modified file must be updated in the catalog"
    );

    // delete
    fs::remove_file(fx._lib.path().join("a.jpg")).unwrap();
    assert!(
        wait_for(|| file_size(&fx, "a.jpg").is_none(), Duration::from_secs(5)),
        "deleted file must vanish from the catalog"
    );
    assert_eq!(count_files(&fx), 1);
}

/// Lifecycle: after `remove_root`, events in that folder trigger no scans.
#[test]
fn removed_root_stops_scanning() {
    let fx = fixture(&[("a.jpg", 10)]);

    fx.handle.remove_root(fx._lib.path());
    write_jpg(&fx._lib.path().join("ignored.jpg"), 55);

    std::thread::sleep(SETTLE);
    assert_eq!(fx.scans(), 0, "removed root must not trigger scans");
    assert_eq!(row_count_for(&fx, "ignored.jpg"), 0);
}

/// Lifecycle: roots added after spawn are watched too.
#[test]
fn added_root_is_watched() {
    let lib_a = tempfile::tempdir().unwrap();
    write_jpg(&lib_a.path().join("a.jpg"), 10);
    let db = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(db.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, lib_a.path(), &AtomicBool::new(false)).unwrap();
    let catalog = Arc::new(Mutex::new(cat));
    let handle = Watcher::spawn(&[lib_a.path().to_path_buf()], catalog.clone(), DEBOUNCE).unwrap();

    let lib_b = tempfile::tempdir().unwrap();
    write_jpg(&lib_b.path().join("b.jpg"), 20);
    scan_folder(&mut catalog.lock().unwrap(), lib_b.path(), &AtomicBool::new(false)).unwrap();

    handle.add_root(lib_b.path()).unwrap();
    write_jpg(&lib_b.path().join("new.jpg"), 30);

    assert!(
        wait_for(
            || {
                catalog
                    .lock()
                    .unwrap()
                    .conn()
                    .query_row::<i64, _, _>(
                        "SELECT COUNT(*) FROM files WHERE filename = 'new.jpg'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap_or(0)
                    == 1
            },
            Duration::from_secs(5)
        ),
        "added root must be watched"
    );
    handle.stop();
}

/// No double-scans: while a scan of a root is in flight, new events for that
/// root must not start a second concurrent scan of it. The worker is serial
/// per root, so a burst landing mid-scan yields a follow-up scan, never a
/// concurrent one. Assert via the coalescing counter + catalog consistency.
#[test]
fn scan_burst_never_duplicates_rows() {
    let fx = fixture(&[]);

    // A storm of writes: many temp files each immediately renamed in.
    for i in 0..10 {
        let tmp = fx._lib.path().join(format!("s{i}.jpg.tmp"));
        write_jpg(&tmp, i + 1);
        fs::rename(&tmp, fx._lib.path().join(format!("s{i}.jpg"))).unwrap();
    }

    assert!(
        wait_for(|| count_files(&fx) == 10, Duration::from_secs(5)),
        "all renames-in must land in the catalog"
    );
    for i in 0..10 {
        assert_eq!(row_count_for(&fx, &format!("s{i}.jpg")), 1, "file s{i}.jpg");
    }
}
