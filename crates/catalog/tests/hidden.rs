//! TASK-03-009: hidden photos and folders (PLAN §2 A, ADR-0007). Library-only:
//! the flag lives in `fb:Hidden` / `.fotobase`, every catalog read path
//! filters on it unless "show hidden" is on, and nothing but the photo's own
//! sidecar (or the folder's `.fotobase`) is ever written.

use std::path::{Path, PathBuf};

use fotobase_catalog::{sidecar_path, Catalog};
use fotobase_metadata::{FolderMeta, Sidecar, FOLDER_META_FILE};

fn add_photo(cat: &mut Catalog, dir: &Path, name: &str) -> (i64, PathBuf) {
    std::fs::create_dir_all(dir).unwrap();
    let original = dir.join(name);
    std::fs::write(&original, format!("fake image bytes of {name}")).unwrap();
    let folder_id = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    let path = original.to_str().unwrap().to_string();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
            rusqlite::params![folder_id, path, name],
        )
        .unwrap();
    (cat.conn().last_insert_rowid(), original)
}

fn open(dir: &Path) -> Catalog {
    Catalog::open(dir.join("catalog.db")).unwrap()
}

fn names(files: Vec<fotobase_catalog::FileInfo>) -> Vec<String> {
    files.into_iter().map(|f| f.filename).collect()
}

fn lib() -> (tempfile::TempDir, Catalog, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let cat = open(tmp.path());
    let photos = tmp.path().join("photos");
    (tmp, cat, photos)
}

#[test]
fn a_hidden_photo_disappears_from_every_read_path() {
    let (_tmp, mut cat, photos) = lib();
    let (keep, _) = add_photo(&mut cat, &photos, "keep.jpg");
    let (secret, _) = add_photo(&mut cat, &photos, "secret.jpg");
    cat.set_rating(keep, 5).unwrap();
    cat.set_rating(secret, 5).unwrap();
    let album = cat.create_album("Trip", "2024-01-02", None, None).unwrap();
    cat.add_to_album(album.id, &[keep, secret]).unwrap();
    let folder_ids: Vec<i64> = cat.list_folders().unwrap().iter().map(|f| f.id).collect();

    assert_eq!(cat.hide_photos(&[secret]).unwrap(), 1);

    assert_eq!(names(cat.list_files(100).unwrap()), ["keep.jpg"]);
    assert_eq!(names(cat.list_files_in_folders(&folder_ids, 100).unwrap()), ["keep.jpg"]);
    assert_eq!(names(cat.list_starred(None, 5, 100).unwrap()), ["keep.jpg"]);
    assert_eq!(cat.search_files("jpg", 100).unwrap(), vec![keep]);
    assert_eq!(names(cat.search_files_detailed("jpg", None, 100).unwrap()), ["keep.jpg"]);
    assert_eq!(names(cat.list_album_files(album.id, 100).unwrap()), ["keep.jpg"]);
    assert_eq!(names(cat.search_album_files(album.id, "jpg", 100).unwrap()), ["keep.jpg"]);
    assert_eq!(cat.list_albums().unwrap()[0].count, 1);
}

#[test]
fn show_hidden_reveals_them_flagged() {
    let (_tmp, mut cat, photos) = lib();
    let (keep, _) = add_photo(&mut cat, &photos, "keep.jpg");
    let (secret, _) = add_photo(&mut cat, &photos, "secret.jpg");
    cat.hide_photos(&[secret]).unwrap();

    assert!(!cat.show_hidden());
    cat.set_show_hidden(true);

    let all = cat.list_files(100).unwrap();
    assert_eq!(names(all.clone()), ["keep.jpg", "secret.jpg"]);
    assert!(!all.iter().find(|f| f.id == keep).unwrap().hidden);
    assert!(all.iter().find(|f| f.id == secret).unwrap().hidden);
    assert_eq!(cat.search_files("secret", 100).unwrap(), vec![secret]);

    cat.set_show_hidden(false);
    assert_eq!(names(cat.list_files(100).unwrap()), ["keep.jpg"]);
}

#[test]
fn hiding_writes_only_the_photos_own_sidecar() {
    let (_tmp, mut cat, photos) = lib();
    let (id, original) = add_photo(&mut cat, &photos, "a.jpg");
    let before = std::fs::read(&original).unwrap();

    cat.hide_photos(&[id]).unwrap();

    assert_eq!(std::fs::read(&original).unwrap(), before);
    let sc = Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap();
    assert!(sc.hidden());

    cat.unhide_photos(&[id]).unwrap();
    let sc = Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap();
    assert!(!sc.hidden());
    assert_eq!(std::fs::read(&original).unwrap(), before);
}

#[test]
fn unhide_restores_the_photo_with_stars_and_tags_intact() {
    let (_tmp, mut cat, photos) = lib();
    let (id, _) = add_photo(&mut cat, &photos, "a.jpg");
    cat.set_rating(id, 4).unwrap();
    cat.add_tag(id, "sunset").unwrap();

    cat.hide_photos(&[id]).unwrap();
    assert!(cat.list_files(10).unwrap().is_empty());
    cat.unhide_photos(&[id]).unwrap();

    let files = cat.list_files(10).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].rating, 4);
    assert_eq!(cat.file_tags(id).unwrap(), vec!["sunset"]);
}

#[test]
fn hide_survives_a_catalog_rebuild() {
    let tmp = tempfile::tempdir().unwrap();
    let photos = tmp.path().join("photos");
    let mut cat = open(tmp.path());
    let (id, _) = add_photo(&mut cat, &photos, "a.jpg");
    cat.hide_photos(&[id]).unwrap();
    drop(cat);
    for name in ["catalog.db", "catalog.db-wal", "catalog.db-shm"] {
        let _ = std::fs::remove_file(tmp.path().join(name));
    }

    let mut cat = open(tmp.path());
    add_photo(&mut cat, &photos, "a.jpg");
    assert_eq!(cat.list_files(10).unwrap().len(), 1, "fresh row is visible until sidecars sync");
    cat.rebuild_from_sidecars(true).unwrap();

    assert!(cat.list_files(10).unwrap().is_empty());
    cat.set_show_hidden(true);
    assert!(cat.list_files(10).unwrap()[0].hidden);
}

#[test]
fn hiding_a_folder_hides_its_photos_and_subfolders_without_touching_sidecars() {
    let (_tmp, mut cat, photos) = lib();
    let family = photos.join("family");
    let kids = family.join("kids");
    add_photo(&mut cat, &photos, "top.jpg");
    let (_, in_family) = add_photo(&mut cat, &family, "f.jpg");
    let (_, in_kids) = add_photo(&mut cat, &kids, "k.jpg");

    assert!(cat.hide_folder(family.to_str().unwrap()).unwrap());

    assert_eq!(names(cat.list_files(10).unwrap()), ["top.jpg"]);
    assert!(cat.search_files("k", 10).unwrap().is_empty());
    let visible: Vec<String> = cat.list_folders().unwrap().into_iter().map(|f| f.path).collect();
    assert_eq!(visible, [photos.to_str().unwrap()]);
    // Folder hide is one `.fotobase` flag: no member sidecar is created.
    assert!(FolderMeta::read(&family).unwrap().hidden);
    assert!(!sidecar_path(in_family.to_str().unwrap()).exists());
    assert!(!sidecar_path(in_kids.to_str().unwrap()).exists());

    cat.set_show_hidden(true);
    let all = cat.list_folders().unwrap();
    assert_eq!(all.len(), 3);
    assert!(all.iter().filter(|f| f.hidden).count() == 2);
    assert_eq!(cat.list_files(10).unwrap().iter().filter(|f| f.hidden).count(), 2);

    cat.set_show_hidden(false);
    assert!(cat.unhide_folder(family.to_str().unwrap()).unwrap());
    assert_eq!(cat.list_files(10).unwrap().len(), 3);
    assert!(!FolderMeta::read(&family).unwrap().hidden);
}

#[test]
fn unhiding_a_parent_keeps_a_separately_hidden_subfolder_hidden() {
    let (_tmp, mut cat, photos) = lib();
    let family = photos.join("family");
    let kids = family.join("kids");
    add_photo(&mut cat, &family, "f.jpg");
    add_photo(&mut cat, &kids, "k.jpg");
    cat.hide_folder(family.to_str().unwrap()).unwrap();
    cat.hide_folder(kids.to_str().unwrap()).unwrap();

    cat.unhide_folder(family.to_str().unwrap()).unwrap();

    assert_eq!(names(cat.list_files(10).unwrap()), ["f.jpg"]);
}

#[test]
fn folder_flags_are_read_back_from_disk_after_a_rebuild() {
    let (_tmp, mut cat, photos) = lib();
    let family = photos.join("family");
    let kids = family.join("kids");
    add_photo(&mut cat, &family, "f.jpg");
    add_photo(&mut cat, &kids, "k.jpg");
    // Another tool (or a previous install) left the flag on disk.
    FolderMeta::set_hidden(&family, true).unwrap();
    assert!(family.join(FOLDER_META_FILE).exists());
    assert_eq!(cat.list_files(10).unwrap().len(), 2, "catalog has not seen the flag yet");

    cat.rebuild_from_sidecars(false).unwrap();

    assert!(cat.list_files(10).unwrap().is_empty());

    // Absent file → not hidden (M10).
    std::fs::remove_file(family.join(FOLDER_META_FILE)).unwrap();
    cat.rebuild_from_sidecars(false).unwrap();
    assert_eq!(cat.list_files(10).unwrap().len(), 2);
}
