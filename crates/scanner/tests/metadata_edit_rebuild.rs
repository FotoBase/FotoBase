//! TASK-03-020 integration: date, GPS, artist and copyright edited in the
//! info pane live in the sidecar, so deleting the catalog and re-scanning
//! restores all four, and the original's bytes never change.

use std::fs;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{sidecar_path, Catalog, GpsEdit, MetadataEdit};
use fotobase_scanner::scan_folder;

#[test]
fn edit_then_delete_catalog_then_rescan_restores_all_four_and_leaves_originals_alone() {
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

    let edit = MetadataEdit {
        taken_at: Some("2024-06-01T09:30:00+02:00".into()),
        gps: Some(GpsEdit::Set { latitude: "52°31'59.9\"N".into(), longitude: "-13.405".into() }),
        artist: Some("Ada Lovelace".into()),
        copyright: Some("© 2024 Ada".into()),
    };
    assert!(cat.edit_metadata(id, &edit).unwrap().is_empty());
    let edited = cat.photo_info(id).unwrap().unwrap();
    drop(cat);

    for ext in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{ext}", db.display()));
    }
    let mut fresh = Catalog::open(&db).unwrap();
    scan_folder(&mut fresh, lib.path(), &AtomicBool::new(false)).unwrap();

    let new_id = fresh.file_id_by_path(a_path).unwrap().unwrap();
    let restored = fresh.photo_info(new_id).unwrap().unwrap();
    assert_eq!(restored.taken_at, edited.taken_at);
    assert_eq!(restored.latitude, edited.latitude);
    assert_eq!(restored.longitude, edited.longitude);
    assert_eq!(restored.artist.as_deref(), Some("Ada Lovelace"));
    assert_eq!(restored.copyright.as_deref(), Some("© 2024 Ada"));

    assert_eq!(fs::read(&a).unwrap(), before, "original untouched");
    assert!(sidecar_path(a_path).exists());
}
