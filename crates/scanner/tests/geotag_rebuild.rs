//! TASK-06-005 integration: geotags written by drag-to-geotag live in the
//! sidecar (the source of truth), so deleting the catalog and re-scanning
//! restores the pins, and the original's bytes never change.

use std::fs;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::Catalog;
use fotobase_scanner::scan_folder;

#[test]
fn geotag_then_delete_catalog_then_rescan_restores_the_pin_and_leaves_the_original_alone() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let a = lib.path().join("a.jpg");
    fs::write(&a, vec![0xff; 64]).unwrap();
    let before = fs::read(&a).unwrap();
    let a_path = a.to_str().unwrap();

    let db = data.path().join("catalog.db");
    let mut cat = Catalog::open(&db).unwrap();
    scan_folder(&mut cat, lib.path(), &AtomicBool::new(false)).unwrap();
    let id = cat.file_id_by_path(a_path).unwrap().unwrap();

    cat.geotag_photos(&[id], -33.8688, 151.2093).unwrap();
    drop(cat);

    for ext in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{ext}", db.display()));
    }
    let mut fresh = Catalog::open(&db).unwrap();
    scan_folder(&mut fresh, lib.path(), &AtomicBool::new(false)).unwrap();
    fotobase_scanner::backfill_hashes(&mut fresh, 10, &AtomicBool::new(false)).unwrap();

    let pins = fresh.geotagged_pins().unwrap();
    assert_eq!(pins.len(), 1, "{pins:?}");
    assert!((pins[0].latitude + 33.8688).abs() < 1e-6);
    assert!((pins[0].longitude - 151.2093).abs() < 1e-6);

    assert_eq!(fs::read(&a).unwrap(), before, "original untouched");
}
