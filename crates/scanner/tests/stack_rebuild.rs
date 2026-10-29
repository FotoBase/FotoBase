//! TASK-03-021 integration: stack membership lives in the sidecars, so
//! deleting the catalog and re-scanning restores the stack — and stacking,
//! switching the top and unstacking never touch an original's bytes.

use std::fs;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{Catalog, FileQuery};
use fotobase_scanner::scan_folder;

fn shown(cat: &Catalog) -> Vec<String> {
    let mut names: Vec<String> = cat
        .query_files(&FileQuery::default(), 100)
        .unwrap()
        .into_iter()
        .map(|f| f.filename)
        .collect();
    names.sort();
    names
}

#[test]
fn stack_two_photos_delete_catalog_rebuild_stack_survives_and_originals_are_unchanged() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let jpg = lib.path().join("IMG_0001.jpg");
    let raw = lib.path().join("IMG_0001.cr2");
    fs::write(&jpg, vec![0xff; 64]).unwrap();
    fs::write(&raw, vec![0x42; 128]).unwrap();
    let (jpg_before, raw_before) = (fs::read(&jpg).unwrap(), fs::read(&raw).unwrap());

    let db = data.path().join("catalog.db");
    let mut cat = Catalog::open(&db).unwrap();
    scan_folder(&mut cat, lib.path(), &AtomicBool::new(false)).unwrap();
    let jpg_id = cat.file_id_by_path(jpg.to_str().unwrap()).unwrap().unwrap();
    let raw_id = cat.file_id_by_path(raw.to_str().unwrap()).unwrap().unwrap();

    let stack_id = cat.stack_photos(&[jpg_id, raw_id]).unwrap();
    cat.set_stack_top(raw_id).unwrap();
    assert_eq!(shown(&cat), ["IMG_0001.cr2"]);
    drop(cat);

    for ext in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{ext}", db.display()));
    }
    let mut fresh = Catalog::open(&db).unwrap();
    scan_folder(&mut fresh, lib.path(), &AtomicBool::new(false)).unwrap();

    assert_eq!(shown(&fresh), ["IMG_0001.cr2"], "the stack and its top came back");
    let members = fresh.stack_members(&stack_id).unwrap();
    let names: Vec<_> = members.iter().map(|f| f.filename.as_str()).collect();
    assert_eq!(names, ["IMG_0001.cr2", "IMG_0001.jpg"]);
    assert_eq!(members[0].stack_count, 2);

    fresh.unstack(&stack_id).unwrap();
    assert_eq!(shown(&fresh), ["IMG_0001.cr2", "IMG_0001.jpg"]);

    assert_eq!(fs::read(&jpg).unwrap(), jpg_before, "jpeg untouched");
    assert_eq!(fs::read(&raw).unwrap(), raw_before, "raw untouched");
}

#[test]
fn a_stack_written_by_another_machine_appears_after_a_scan() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let a = lib.path().join("a.jpg");
    let b = lib.path().join("b.jpg");
    fs::write(&a, vec![1; 8]).unwrap();
    fs::write(&b, vec![2; 8]).unwrap();

    // Stack on one catalog, then hand the folder (sidecars included) to a
    // second, empty catalog — the Syncthing case.
    let mut first = Catalog::open(data.path().join("first.db")).unwrap();
    scan_folder(&mut first, lib.path(), &AtomicBool::new(false)).unwrap();
    let ids: Vec<i64> = [&a, &b]
        .iter()
        .map(|p| first.file_id_by_path(p.to_str().unwrap()).unwrap().unwrap())
        .collect();
    first.stack_photos(&ids).unwrap();

    let mut second = Catalog::open(data.path().join("second.db")).unwrap();
    scan_folder(&mut second, lib.path(), &AtomicBool::new(false)).unwrap();

    assert_eq!(shown(&second), ["a.jpg"]);
}
