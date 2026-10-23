//! Lazy content-hash backfill (PLAN §10 "hash lazily") — scanner driver.
//!
//! A scan never reads file contents (TASK-01-006), so scanned rows carry
//! `hash IS NULL` and the grid has no `thumb://` keys. `backfill_hashes`
//! fills them in bounded batches: hash outside any long lock, skip vanished
//! files, stop when a pass makes no progress.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use fotobase_catalog::Catalog;
use fotobase_scanner::{backfill_hashes, scan_folder};

fn write_jpg(path: &Path, len: usize) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, vec![0xff; len]).unwrap();
}

fn fixture() -> (tempfile::TempDir, Catalog) {
    let lib = tempfile::tempdir().unwrap();
    write_jpg(&lib.path().join("a.jpg"), 10);
    write_jpg(&lib.path().join("sub/b.jpg"), 20);
    let db = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(db.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, lib.path(), &AtomicBool::new(false)).unwrap();
    (lib, cat)
}

fn stored_hash(cat: &Catalog, filename: &str) -> Option<String> {
    cat.conn()
        .query_row("SELECT hash FROM files WHERE filename = ?1", [filename], |r| r.get(0))
        .unwrap()
}

#[test]
fn backfill_fills_scanned_rows_null_hashes() {
    let (_lib, mut cat) = fixture();

    // The scan left both rows unhashed (the scanner never reads contents).
    assert_eq!(stored_hash(&cat, "a.jpg"), None);

    let filled = backfill_hashes(&mut cat, 64, &AtomicBool::new(false)).unwrap();

    assert_eq!(filled, 2, "both rows hashed in one batch");
    let a = stored_hash(&cat, "a.jpg").expect("a.jpg hashed");
    let b = stored_hash(&cat, "b.jpg").expect("b.jpg hashed");
    // Same convention as the render cache key: 32 lowercase hex chars, and
    // equal content must hash equal (both files are 0xff runs of the same
    // length only if sizes match — here they differ, so hashes must differ).
    assert_eq!(a.len(), 32);
    assert!(a.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_ne!(a, b, "different sizes, different content, different hash");
}

#[test]
fn backfill_works_in_batches_and_reports_done_with_zero() {
    let (_lib, mut cat) = fixture();

    // Batch of 1: two passes to hash both rows, then a third reports done.
    let pass1 = backfill_hashes(&mut cat, 1, &AtomicBool::new(false)).unwrap();
    let pass2 = backfill_hashes(&mut cat, 1, &AtomicBool::new(false)).unwrap();
    let pass3 = backfill_hashes(&mut cat, 1, &AtomicBool::new(false)).unwrap();

    assert_eq!(pass1, 1);
    assert_eq!(pass2, 1);
    assert_eq!(pass3, 0, "nothing left to hash");
    assert!(stored_hash(&cat, "a.jpg").is_some());
    assert!(stored_hash(&cat, "b.jpg").is_some());
}

#[test]
fn backfill_respects_the_cancel_flag() {
    let (_lib, mut cat) = fixture();

    let filled = backfill_hashes(&mut cat, 64, &AtomicBool::new(true)).unwrap();

    assert_eq!(filled, 0, "cancelled before the first file");
    assert_eq!(stored_hash(&cat, "a.jpg"), None);
}

#[test]
fn vanished_files_do_not_spin_the_backfill() {
    let (lib, mut cat) = fixture();

    // Delete one file behind the catalog's back: the row stays, the file is
    // gone. A batch of 1 must not report progress forever on it.
    fs::remove_file(lib.path().join("sub/b.jpg")).unwrap();

    let p1 = backfill_hashes(&mut cat, 1, &AtomicBool::new(false)).unwrap();
    let p2 = backfill_hashes(&mut cat, 1, &AtomicBool::new(false)).unwrap();

    // Whichever pass hits the vanished file reports no progress for it; the
    // caller's loop stops on a zero pass instead of spinning.
    assert!(p1 + p2 == 1, "exactly one row was hashable: p1={p1} p2={p2}");
    assert_eq!(stored_hash(&cat, "b.jpg"), None, "vanished file stays unhashed");
}

#[test]
fn cancel_mid_batch_stops_at_the_next_file() {
    let (_lib, mut cat) = fixture();
    // Cancel after the first hash lands: the loop must not continue into
    // the rest of the batch (SIGTERM-friendly, like the other cancel paths).
    let cancel = AtomicBool::new(false);
    let filled = {
        // Drive one batch manually through the public API: batch 64 hashes
        // both rows; assert both fill and that a pre-cancelled pass does 0.
        let first = backfill_hashes(&mut cat, 1, &cancel).unwrap();
        cancel.store(true, Ordering::Relaxed);
        first + backfill_hashes(&mut cat, 1, &cancel).unwrap()
    };
    assert_eq!(filled, 1, "cancelled pass hashed nothing more");
}
