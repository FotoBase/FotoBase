//! TASK-03-012: catalog trash (PLAN §6 P2a). Remove-from-album, hide and
//! unlink are soft-deleted into a `trash` table with a restore payload;
//! undo replays the reverse operation through the same mutation paths, the
//! undo window expires entries by timestamp, and no original is ever touched.

use std::path::{Path, PathBuf};

use fotobase_catalog::{sidecar_path, Catalog, ScanMode, TrashAction, DEFAULT_TRASH_WINDOW_DAYS};
use fotobase_metadata::Sidecar;

const DAY: i64 = 86_400;

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

fn lib() -> (tempfile::TempDir, Catalog, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
    let photos = tmp.path().join("photos");
    (tmp, cat, photos)
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

/// Pretend an entry was trashed `days` ago.
fn age(cat: &Catalog, days: i64) {
    cat.conn().execute("UPDATE trash SET created_at = created_at - ?1", [days * DAY]).unwrap();
}

fn sidecar_albums(original: &Path) -> Vec<String> {
    Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap().albums()
}

#[test]
fn the_default_window_is_thirty_days() {
    assert_eq!(DEFAULT_TRASH_WINDOW_DAYS, 30);
}

// ---- remove from album ------------------------------------------------------

#[test]
fn removing_from_an_album_lands_in_the_trash_and_undo_restores_membership() {
    let (_tmp, mut cat, photos) = lib();
    let (id, original) = add_photo(&mut cat, &photos, "a.jpg");
    let album = cat.create_album("Trip", "2024-01-02", None, None).unwrap();
    cat.add_to_album(album.id, &[id]).unwrap();

    assert_eq!(cat.trash_remove_from_album(album.id, &[id]).unwrap(), 1);

    assert!(cat.album_file_ids(album.id).unwrap().is_empty());
    let entries = cat.trash_list().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, TrashAction::AlbumRemove);
    assert_eq!(entries[0].path, original.to_str().unwrap());
    assert_eq!(entries[0].name, "a.jpg");
    assert_eq!(entries[0].detail.as_deref(), Some("Trip"));
    assert!((entries[0].created_at - now()).abs() < 5);

    let report = cat.undo_trash(entries[0].id).unwrap();

    assert_eq!((report.restored, report.skipped), (1, 0));
    assert_eq!(cat.album_file_ids(album.id).unwrap(), vec![id]);
    assert_eq!(sidecar_albums(&original), vec![album.uuid]);
    assert!(cat.trash_list().unwrap().is_empty());
}

#[test]
fn removing_a_non_member_records_nothing() {
    let (_tmp, mut cat, photos) = lib();
    let (id, _) = add_photo(&mut cat, &photos, "a.jpg");
    let album = cat.create_album("Trip", "2024-01-02", None, None).unwrap();

    assert_eq!(cat.trash_remove_from_album(album.id, &[id]).unwrap(), 0);
    assert!(cat.trash_list().unwrap().is_empty());
}

#[test]
fn undoing_into_a_deleted_album_is_skipped_and_the_entry_is_dropped() {
    let (_tmp, mut cat, photos) = lib();
    let (id, _) = add_photo(&mut cat, &photos, "a.jpg");
    let album = cat.create_album("Trip", "2024-01-02", None, None).unwrap();
    cat.add_to_album(album.id, &[id]).unwrap();
    cat.trash_remove_from_album(album.id, &[id]).unwrap();
    cat.delete_album(album.id).unwrap();

    let entry = cat.trash_list().unwrap().remove(0);
    let report = cat.undo_trash(entry.id).unwrap();

    assert_eq!((report.restored, report.skipped), (0, 1));
    assert!(cat.trash_list().unwrap().is_empty());
}

// ---- hide -------------------------------------------------------------------

#[test]
fn hide_lands_in_the_trash_and_undo_makes_the_photo_visible_with_stars_and_tags() {
    let (_tmp, mut cat, photos) = lib();
    let (id, _) = add_photo(&mut cat, &photos, "a.jpg");
    cat.set_rating(id, 5).unwrap();
    cat.add_tag(id, "sunset").unwrap();

    cat.hide_photos(&[id]).unwrap();
    assert!(cat.list_files(10).unwrap().is_empty());
    let entry = cat.trash_list().unwrap().remove(0);
    assert_eq!(entry.action, TrashAction::Hide);
    assert_eq!(entry.name, "a.jpg");

    cat.undo_trash(entry.id).unwrap();

    let files = cat.list_files(10).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].rating, 5);
    assert_eq!(cat.file_tags(id).unwrap(), vec!["sunset"]);
    assert!(cat.trash_list().unwrap().is_empty());
}

#[test]
fn hiding_an_already_hidden_photo_adds_no_second_entry() {
    let (_tmp, mut cat, photos) = lib();
    let (id, _) = add_photo(&mut cat, &photos, "a.jpg");
    assert_eq!(cat.hide_photos(&[id]).unwrap(), 1);
    assert_eq!(cat.hide_photos(&[id]).unwrap(), 0);
    assert_eq!(cat.trash_list().unwrap().len(), 1);
}

#[test]
fn unhiding_directly_drops_the_matching_trash_entry() {
    let (_tmp, mut cat, photos) = lib();
    let (id, _) = add_photo(&mut cat, &photos, "a.jpg");
    cat.hide_photos(&[id]).unwrap();
    cat.unhide_photos(&[id]).unwrap();
    assert!(cat.trash_list().unwrap().is_empty());
}

#[test]
fn hiding_a_folder_is_trashed_and_undo_unhides_it() {
    let (_tmp, mut cat, photos) = lib();
    add_photo(&mut cat, &photos, "a.jpg");

    cat.hide_folder(photos.to_str().unwrap()).unwrap();
    assert!(cat.list_files(10).unwrap().is_empty());
    let entry = cat.trash_list().unwrap().remove(0);
    assert_eq!(entry.action, TrashAction::Hide);
    assert_eq!(entry.path, photos.to_str().unwrap());
    assert_eq!(entry.detail.as_deref(), Some("folder"));

    cat.undo_trash(entry.id).unwrap();
    assert_eq!(cat.list_files(10).unwrap().len(), 1);
}

// ---- unlink -----------------------------------------------------------------

#[test]
fn unlinking_a_folder_is_trashed_and_undo_reseeds_the_tree_for_a_rescan() {
    let (_tmp, mut cat, photos) = lib();
    let nested = photos.join("nested");
    add_photo(&mut cat, &photos, "a.jpg");
    add_photo(&mut cat, &nested, "b.jpg");
    cat.set_folder_scan_mode(photos.to_str().unwrap(), ScanMode::Always).unwrap();
    let root = photos.to_str().unwrap();

    assert_eq!(cat.unlink_folder(root).unwrap(), 2);

    assert!(cat.list_folders().unwrap().is_empty());
    assert!(cat.list_files(10).unwrap().is_empty());
    let entry = cat.trash_list().unwrap().remove(0);
    assert_eq!(entry.action, TrashAction::Unlink);
    assert_eq!(entry.path, root);

    let report = cat.undo_trash(entry.id).unwrap();

    assert_eq!(report.restored, 1);
    assert_eq!(report.rescan, vec![root.to_string()]);
    let folders = cat.list_folders().unwrap();
    assert_eq!(folders.len(), 2);
    let top = folders.iter().find(|f| f.path == root).unwrap();
    assert_eq!(top.scan_mode, ScanMode::Always);
    assert!(cat.trash_list().unwrap().is_empty());
}

#[test]
fn unlinking_an_unknown_folder_records_nothing() {
    let (_tmp, mut cat, photos) = lib();
    assert_eq!(cat.unlink_folder(photos.to_str().unwrap()).unwrap(), 0);
    assert!(cat.trash_list().unwrap().is_empty());
}

#[test]
fn unlinking_touches_nothing_on_disk() {
    let (_tmp, mut cat, photos) = lib();
    let (id, original) = add_photo(&mut cat, &photos, "a.jpg");
    cat.set_rating(id, 3).unwrap();
    let listing = |dir: &Path| {
        let mut v: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap())
            })
            .collect();
        v.sort();
        v
    };
    let before = listing(&photos);

    let entry_id = {
        cat.unlink_folder(photos.to_str().unwrap()).unwrap();
        cat.trash_list().unwrap().remove(0).id
    };
    cat.undo_trash(entry_id).unwrap();

    assert_eq!(listing(&photos), before);
    assert!(original.exists());
}

// ---- list / undo-all ---------------------------------------------------------

#[test]
fn the_trash_lists_newest_first_and_undo_all_is_per_action_type() {
    let (_tmp, mut cat, photos) = lib();
    let (a, orig_a) = add_photo(&mut cat, &photos, "a.jpg");
    let (b, _) = add_photo(&mut cat, &photos, "b.jpg");
    let album = cat.create_album("Trip", "2024-01-02", None, None).unwrap();
    cat.add_to_album(album.id, &[a, b]).unwrap();
    cat.trash_remove_from_album(album.id, &[a]).unwrap();
    cat.hide_photos(&[a, b]).unwrap();

    let all = cat.trash_list().unwrap();
    assert_eq!(all.len(), 3);
    assert!(all.windows(2).all(|w| w[0].id > w[1].id), "newest first");

    let report = cat.undo_trash_all(TrashAction::Hide).unwrap();

    assert_eq!(report.restored, 2);
    assert_eq!(cat.list_files(10).unwrap().len(), 2);
    let left = cat.trash_list().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].action, TrashAction::AlbumRemove);
    assert!(sidecar_albums(&orig_a).is_empty(), "album removal not undone by undo-all hide");
}

#[test]
fn undoing_an_unknown_entry_is_an_error() {
    let (_tmp, mut cat, _photos) = lib();
    assert!(cat.undo_trash(999).is_err());
}

// ---- expiry -----------------------------------------------------------------

#[test]
fn the_sweep_drops_entries_past_the_window_and_keeps_the_rest() {
    let (_tmp, mut cat, photos) = lib();
    let (a, _) = add_photo(&mut cat, &photos, "a.jpg");
    let (b, _) = add_photo(&mut cat, &photos, "b.jpg");
    cat.hide_photos(&[a]).unwrap();
    age(&cat, 40);
    cat.hide_photos(&[b]).unwrap();

    assert_eq!(cat.sweep_trash(30, now()).unwrap(), 1);

    let left = cat.trash_list().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].name, "b.jpg");
}

#[test]
fn the_window_is_configurable() {
    let (_tmp, mut cat, photos) = lib();
    let (a, _) = add_photo(&mut cat, &photos, "a.jpg");
    cat.hide_photos(&[a]).unwrap();
    age(&cat, 10);

    assert_eq!(cat.sweep_trash(30, now()).unwrap(), 0);
    assert_eq!(cat.sweep_trash(7, now()).unwrap(), 1);
    assert!(cat.trash_list().unwrap().is_empty());
}

#[test]
fn an_expired_hide_stays_hidden_only_the_undo_is_gone() {
    let (_tmp, mut cat, photos) = lib();
    let (a, _) = add_photo(&mut cat, &photos, "a.jpg");
    cat.hide_photos(&[a]).unwrap();
    age(&cat, 31);

    cat.sweep_trash(30, now()).unwrap();

    assert!(cat.list_files(10).unwrap().is_empty());
}
