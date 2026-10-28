//! TASK-03-005 integration: stars live in the sidecar, so deleting the
//! catalog and re-scanning restores them (PLAN §6 P2a acceptance).

use std::fs;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{sidecar_path, Catalog};
use fotobase_scanner::{scan_folder, scan_folder_with, ScanOptions};

#[test]
fn star_then_delete_catalog_then_rescan_still_starred_and_originals_untouched() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let photo = lib.path().join("a.jpg");
    let other = lib.path().join("b.jpg");
    fs::write(&photo, vec![0xff; 64]).unwrap();
    fs::write(&other, vec![0xee; 32]).unwrap();
    let photo_before = fs::read(&photo).unwrap();
    let other_before = fs::read(&other).unwrap();

    let db = data.path().join("catalog.db");
    let mut cat = Catalog::open(&db).unwrap();
    scan_folder(&mut cat, lib.path(), &AtomicBool::new(false)).unwrap();
    let id = cat.file_id_by_path(photo.to_str().unwrap()).unwrap().unwrap();
    cat.set_rating(id, 5).unwrap();
    drop(cat);

    // Catalog gone (WAL/SHM siblings included).
    for ext in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{ext}", db.display()));
    }

    let mut fresh = Catalog::open(&db).unwrap();
    scan_folder(&mut fresh, lib.path(), &AtomicBool::new(false)).unwrap();

    let starred = fresh.list_starred(None, 5, usize::MAX).unwrap();
    assert_eq!(starred.len(), 1, "{starred:?}");
    assert_eq!(starred[0].filename, "a.jpg");
    assert_eq!(starred[0].rating, 5);

    assert_eq!(fs::read(&photo).unwrap(), photo_before, "original untouched");
    assert_eq!(fs::read(&other).unwrap(), other_before, "original untouched");
    assert!(sidecar_path(photo.to_str().unwrap()).exists());
    assert!(!sidecar_path(other.to_str().unwrap()).exists(), "no sidecar for the unstarred photo");
}

#[test]
fn full_rebuild_scan_re_reads_sidecars() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let photo = lib.path().join("a.jpg");
    fs::write(&photo, vec![0xff; 64]).unwrap();

    let mut cat = Catalog::open(data.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, lib.path(), &AtomicBool::new(false)).unwrap();
    let id = cat.file_id_by_path(photo.to_str().unwrap()).unwrap().unwrap();
    cat.set_rating(id, 5).unwrap();
    cat.conn().execute("UPDATE metadata SET rating = NULL", []).unwrap();
    cat.conn().execute("UPDATE files SET sidecar_mtime = NULL, sidecar_size = NULL", []).unwrap();

    scan_folder_with(
        &mut cat,
        lib.path(),
        &AtomicBool::new(false),
        &ScanOptions { full: true },
        &|_| {},
    )
    .unwrap();

    assert_eq!(cat.list_starred(None, 5, usize::MAX).unwrap().len(), 1);
}

#[test]
fn malformed_sidecar_does_not_fail_the_scan() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let photo = lib.path().join("a.jpg");
    fs::write(&photo, vec![0xff; 64]).unwrap();
    fs::write(sidecar_path(photo.to_str().unwrap()), "<x:xmpmeta><rdf:RDF>").unwrap();

    let mut cat = Catalog::open(data.path().join("catalog.db")).unwrap();
    let report = scan_folder(&mut cat, lib.path(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.added, 1);
    assert!(cat.list_starred(None, 1, usize::MAX).unwrap().is_empty());
}
