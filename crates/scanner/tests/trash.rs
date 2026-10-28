//! TASK-03-012 / TASK-03-009 scanner integration: an undone unlink comes back
//! through a rescan with its user data (sidecars win, PLAN §5), and the
//! scanner inherits the folder-level hide flag from `.fotobase` (M10).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{sidecar_path, Catalog, TrashAction};
use fotobase_metadata::{FolderMeta, Sidecar, FOLDER_META_FILE};
use fotobase_scanner::scan_folder;

static CANCEL: AtomicBool = AtomicBool::new(false);

fn photo(dir: &Path, name: &str, rating: u8) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let original = dir.join(name);
    std::fs::write(&original, format!("bytes of {name}")).unwrap();
    let sc = sidecar_path(original.to_str().unwrap());
    let mut side = Sidecar::read(&sc).unwrap();
    side.set_rating(rating);
    side.write(&sc).unwrap();
    original
}

fn names(cat: &Catalog) -> Vec<String> {
    cat.list_files(100).unwrap().into_iter().map(|f| f.filename).collect()
}

#[test]
fn undoing_an_unlink_and_rescanning_restores_photos_with_stars_and_tags() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("photos");
    photo(&root, "a.jpg", 5);
    photo(&root.join("sub"), "b.jpg", 3);
    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, &root, &CANCEL).unwrap();
    let a = cat.file_id_by_path(root.join("a.jpg").to_str().unwrap()).unwrap().unwrap();
    cat.add_tag(a, "keeper").unwrap();
    let root_str = root.to_str().unwrap();

    assert_eq!(cat.unlink_folder(root_str).unwrap(), 2);
    assert!(names(&cat).is_empty());

    let entry = cat.trash_list().unwrap().remove(0);
    assert_eq!(entry.action, TrashAction::Unlink);
    let report = cat.undo_trash(entry.id).unwrap();
    for dir in &report.rescan {
        scan_folder(&mut cat, Path::new(dir), &CANCEL).unwrap();
    }

    assert_eq!(names(&cat), ["a.jpg", "b.jpg"]);
    let files = cat.list_files(10).unwrap();
    assert_eq!(files.iter().map(|f| f.rating).collect::<Vec<_>>(), [5, 3]);
    let a = cat.file_id_by_path(root.join("a.jpg").to_str().unwrap()).unwrap().unwrap();
    assert_eq!(cat.file_tags(a).unwrap(), vec!["keeper"]);
    assert!(cat.trash_list().unwrap().is_empty());
}

#[test]
fn the_scan_inherits_a_hidden_folder_flag_and_clears_it_when_the_file_is_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("photos");
    let private = root.join("private");
    photo(&root, "a.jpg", 0);
    photo(&private, "p.jpg", 0);
    photo(&private.join("deep"), "d.jpg", 0);
    FolderMeta::set_hidden(&private, true).unwrap();
    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();

    scan_folder(&mut cat, &root, &CANCEL).unwrap();

    assert_eq!(names(&cat), ["a.jpg"]);
    cat.set_show_hidden(true);
    assert_eq!(cat.list_files(10).unwrap().iter().filter(|f| f.hidden).count(), 2);
    cat.set_show_hidden(false);

    // Absent file → not hidden (M10).
    std::fs::remove_file(private.join(FOLDER_META_FILE)).unwrap();
    scan_folder(&mut cat, &root, &CANCEL).unwrap();
    assert_eq!(cat.list_files(10).unwrap().len(), 3);
}

#[test]
fn hide_survives_deleting_the_catalog_and_rescanning() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("photos");
    let secret = photo(&root, "secret.jpg", 4);
    photo(&root, "open.jpg", 4);
    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, &root, &CANCEL).unwrap();
    let id = cat.file_id_by_path(secret.to_str().unwrap()).unwrap().unwrap();
    cat.hide_photos(&[id]).unwrap();
    drop(cat);
    for name in ["catalog.db", "catalog.db-wal", "catalog.db-shm"] {
        let _ = std::fs::remove_file(tmp.path().join(name));
    }

    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, &root, &CANCEL).unwrap();

    assert_eq!(names(&cat), ["open.jpg"]);
}
