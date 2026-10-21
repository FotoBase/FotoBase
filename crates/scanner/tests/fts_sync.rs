//! Scanner <-> FTS sync tests (TASK-01-008).
//!
//! A scanned file must be immediately searchable, and a removed file must
//! stop matching, without any manual index maintenance.

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::Catalog;
use fotobase_scanner::scan_folder;

fn write_jpg(path: &Path, len: usize) {
    fs::write(path, vec![0xff; len]).unwrap();
}

#[test]
fn scanned_files_are_immediately_searchable() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();

    fs::create_dir_all(tmp.path().join("2024")).unwrap();
    write_jpg(&tmp.path().join("2024/beach.jpg"), 10);
    let report = scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.added, 1);

    assert_eq!(cat.search_files("beach", 100).unwrap().len(), 1);
    assert_eq!(cat.search_files("2024", 100).unwrap().len(), 1);
    assert!(cat.search_files("xyzzy", 100).unwrap().is_empty());

    // New file appears on the next scan and is searchable right away.
    write_jpg(&tmp.path().join("2024/sunset.png"), 10);
    scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();
    assert_eq!(cat.search_files("sunset", 100).unwrap().len(), 1);

    // Removed file stops matching.
    fs::remove_file(tmp.path().join("2024/beach.jpg")).unwrap();
    scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();
    assert!(cat.search_files("beach", 100).unwrap().is_empty());
    assert_eq!(cat.search_files("sunset", 100).unwrap().len(), 1);
}

#[test]
fn no_duplicate_fts_rows_on_unchanged_rescan() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();

    fs::create_dir_all(tmp.path().join("2024")).unwrap();
    write_jpg(&tmp.path().join("2024/beach.jpg"), 10);
    scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();
    scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();

    let fts_rows: i64 =
        cat.conn().query_row("SELECT COUNT(*) FROM files_fts", [], |r| r.get(0)).unwrap();
    assert_eq!(fts_rows, 1);
}
