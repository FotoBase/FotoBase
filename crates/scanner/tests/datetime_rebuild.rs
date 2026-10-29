//! TASK-03-019 integration: adjusted capture times live in the sidecar, so
//! deleting the catalog and re-scanning restores them (PLAN §6 P2a), and the
//! originals are never touched.

use std::fs;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{sidecar_path, Catalog, TimeAdjustment};
use fotobase_scanner::scan_folder;

fn taken_at(cat: &Catalog, path: &str) -> Option<String> {
    cat.conn()
        .query_row("SELECT taken_at FROM files WHERE path = ?1", [path], |r| r.get(0))
        .unwrap()
}

#[test]
fn adjust_then_delete_catalog_then_rescan_keeps_taken_at_and_originals_untouched() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let a = lib.path().join("a.jpg");
    let b = lib.path().join("b.jpg");
    fs::write(&a, vec![0xff; 64]).unwrap();
    fs::write(&b, vec![0xee; 32]).unwrap();
    let (a_before, b_before) = (fs::read(&a).unwrap(), fs::read(&b).unwrap());
    let (a_path, b_path) = (a.to_str().unwrap(), b.to_str().unwrap());

    let db = data.path().join("catalog.db");
    let mut cat = Catalog::open(&db).unwrap();
    scan_folder(&mut cat, lib.path(), &AtomicBool::new(false)).unwrap();
    let ids = [
        cat.file_id_by_path(a_path).unwrap().unwrap(),
        cat.file_id_by_path(b_path).unwrap().unwrap(),
    ];
    // Camera clocks: seed a scan-time capture time, then shift a by +1 day.
    cat.conn()
        .execute(
            "UPDATE files SET taken_at = '2024-06-01T12:00:00+02:00' WHERE path = ?1",
            [a_path],
        )
        .unwrap();
    let report = cat
        .adjust_capture_times(
            &ids[..1],
            &TimeAdjustment::Shift { seconds: 86_400 },
            &AtomicBool::new(false),
            &|_, _| {},
        )
        .unwrap();
    assert_eq!(report.adjusted, 1);
    let adjusted = taken_at(&cat, a_path);
    assert_eq!(adjusted.as_deref(), Some("2024-06-02T12:00:00+02:00"));
    drop(cat);

    // Catalog gone (WAL/SHM siblings included).
    for ext in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{ext}", db.display()));
    }

    let mut fresh = Catalog::open(&db).unwrap();
    scan_folder(&mut fresh, lib.path(), &AtomicBool::new(false)).unwrap();

    assert_eq!(taken_at(&fresh, a_path), adjusted, "adjusted time restored from the sidecar");
    assert_eq!(taken_at(&fresh, b_path), None, "untouched photo has no invented date");

    assert_eq!(fs::read(&a).unwrap(), a_before, "original untouched");
    assert_eq!(fs::read(&b).unwrap(), b_before, "original untouched");
    assert!(sidecar_path(a_path).exists());
    assert!(!sidecar_path(b_path).exists(), "no sidecar for the unadjusted photo");
}
