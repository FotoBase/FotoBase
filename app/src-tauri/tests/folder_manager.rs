//! TASK-01-016 integration: the Folder Manager's backend half.
//!
//! "Scan Always" hands a folder to the `notify` watcher (01-007): a file
//! created inside must appear in the catalog **without any manual scan**.
//! "Remove from catalog" drops the folder's catalog rows only — the files on
//! disk are never touched (PLAN §2A: the library mirrors real disk folders).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fotobase_catalog::{Catalog, ScanMode};
use fotobase_lib::folder_manager::FolderManager;
use fotobase_scanner::scan_folder;
use std::sync::atomic::AtomicBool;

// Same reasoning as crates/scanner/tests/watcher.rs: debounce windows must
// dwarf CI scheduling stalls.
const DEBOUNCE: Duration = Duration::from_millis(300);
const SETTLE: Duration = Duration::from_millis(1200);

fn write_jpg(path: &Path, len: usize) {
    fs::write(path, vec![0xff; len]).unwrap();
}

fn wait_for(mut cond: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    cond()
}

fn normalize(path: &Path) -> String {
    // Native-separator path, like the scanner stores them (Windows-safe).
    path.components().collect::<PathBuf>().to_string_lossy().into_owned()
}

/// Seed one photo, scan it into the catalog, then open a FolderManager with
/// that folder *not* yet watched.
fn fixture() -> (tempfile::TempDir, tempfile::TempDir, FolderManager) {
    let lib = tempfile::tempdir().unwrap();
    write_jpg(&lib.path().join("a.jpg"), 10);
    let db = tempfile::tempdir().unwrap();
    let catalog_path = db.path().join("catalog.db");
    let mut cat = Catalog::open(&catalog_path).unwrap();
    scan_folder(&mut cat, lib.path(), &AtomicBool::new(false)).unwrap();
    drop(cat);
    let fm = FolderManager::open(&catalog_path, DEBOUNCE).unwrap();
    (lib, db, fm)
}

fn count_files(catalog_path: &Path) -> i64 {
    let cat = Catalog::open(catalog_path).unwrap();
    cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap()
}

#[test]
fn scan_always_picks_up_new_files_without_a_manual_scan() {
    let (lib, db, fm) = fixture();
    let catalog_path = db.path().join("catalog.db");

    fm.set_mode(&normalize(lib.path()), ScanMode::Always).expect("apply Scan Always");

    // A photo appears on disk after the mode was applied…
    write_jpg(&lib.path().join("late.jpg"), 20);

    // …and the watcher gets it into the catalog on its own.
    assert!(
        wait_for(|| count_files(&catalog_path) == 2, SETTLE),
        "new file never reached the catalog"
    );
    fm.stop();
}

#[test]
fn scan_once_scans_now_but_does_not_watch() {
    let (lib, db, fm) = fixture();
    let catalog_path = db.path().join("catalog.db");

    fm.set_mode(&normalize(lib.path()), ScanMode::Once).expect("apply Scan Once");
    let scans_after_apply = fm.scan_count();

    write_jpg(&lib.path().join("late.jpg"), 20);
    // No watcher running for this folder: the file must NOT appear on its own.
    std::thread::sleep(SETTLE);
    assert_eq!(count_files(&catalog_path), 1, "Scan Once must not watch");

    // An explicit rescan brings it in.
    let report = fm.scan_now(&normalize(lib.path())).expect("scan now");
    assert_eq!(report.added, 1);
    assert_eq!(count_files(&catalog_path), 2);
    assert_eq!(fm.scan_count(), scans_after_apply, "Scan Once never arms the watcher");
    fm.stop();
}

#[test]
fn remove_drops_catalog_rows_but_keeps_files_on_disk() {
    let (lib, _db, fm) = fixture();
    let catalog_path = _db.path().join("catalog.db");

    let removed = fm.remove(&normalize(lib.path())).expect("remove folder");
    assert!(removed >= 1, "the folder row (and nested rows) are gone");

    // Catalog is empty; the disk is untouched.
    assert_eq!(count_files(&catalog_path), 0);
    assert!(lib.path().join("a.jpg").exists(), "files stay on disk");
    assert!(Catalog::open(&catalog_path).unwrap().list_folders().unwrap().is_empty());

    // Removing a folder that is not in the catalog is a no-op, not an error.
    assert_eq!(fm.remove(&normalize(lib.path())).unwrap(), 0);
    fm.stop();
}

#[test]
fn switching_from_always_to_once_stops_watching() {
    let (lib, db, fm) = fixture();
    let catalog_path = db.path().join("catalog.db");
    let root = normalize(lib.path());

    fm.set_mode(&root, ScanMode::Always).expect("watch");
    fm.set_mode(&root, ScanMode::Once).expect("un-watch");

    // Drain the closing scan `add_root` enqueued: it was legitimately armed
    // while the folder was still watched, and a scan that reflects disk
    // state is not "an event after un-watching". What must never happen is
    // a scan *starting* after the un-watch.
    assert!(wait_for(|| fm.scan_count() >= 1, SETTLE), "closing scan never ran");

    write_jpg(&lib.path().join("unwatched.jpg"), 20);
    std::thread::sleep(SETTLE);
    assert_eq!(count_files(&catalog_path), 1, "no events after un-watching");

    // The catalog still records the folder with its new mode.
    let cat = Catalog::open(&catalog_path).unwrap();
    let folders = cat.list_folders().unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].scan_mode, ScanMode::Once);
    fm.stop();
}
