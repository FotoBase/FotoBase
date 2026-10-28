//! TASK-03-008: virtual albums. Membership is written to `fb:Albums` in each
//! member's sidecar (sidecar first, catalog mirror second); album definitions
//! live in `albums.json` next to the catalog so everything rebuilds after the
//! catalog is deleted (PLAN §5, docs/sidecar-merge-on-write.md §2).

use std::path::{Path, PathBuf};

use fotobase_catalog::{sidecar_path, Catalog, SyncError};
use fotobase_metadata::Sidecar;

fn add_photo(cat: &mut Catalog, dir: &Path, name: &str) -> (i64, PathBuf) {
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

fn sidecar_albums(original: &Path) -> Vec<String> {
    Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap().albums()
}

fn open(dir: &Path) -> Catalog {
    Catalog::open(dir.join("catalog.db")).unwrap()
}

fn delete_catalog(dir: &Path) {
    for name in ["catalog.db", "catalog.db-wal", "catalog.db-shm"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

#[test]
fn create_album_stores_name_date_place_and_description() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());

    let album = cat
        .create_album("Summer 2024", "2024-07-14", Some("Lisbon"), Some("Two weeks by the sea"))
        .unwrap();

    assert_eq!(album.name, "Summer 2024");
    assert_eq!(album.date.as_deref(), Some("2024-07-14"));
    assert_eq!(album.place.as_deref(), Some("Lisbon"));
    assert_eq!(album.description.as_deref(), Some("Two weeks by the sea"));
    assert_eq!(album.count, 0);
    assert_eq!(album.uuid.len(), 36, "uuid: {}", album.uuid);
    assert_eq!(cat.list_albums().unwrap(), vec![album]);
}

#[test]
fn place_and_description_are_optional_and_blank_means_none() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());

    let album = cat.create_album("  Plain  ", "2024-01-02", Some("  "), None).unwrap();

    assert_eq!(album.name, "Plain");
    assert_eq!(album.place, None);
    assert_eq!(album.description, None);
}

#[test]
fn create_album_rejects_an_empty_name_and_a_bad_date() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());

    assert!(matches!(
        cat.create_album("   ", "2024-01-02", None, None),
        Err(SyncError::EmptyAlbumName)
    ));
    for bad in ["", "yesterday", "2024-13-01", "2024-02-30", "24-01-02"] {
        assert!(
            matches!(cat.create_album("x", bad, None, None), Err(SyncError::InvalidAlbumDate(_))),
            "{bad:?} must be rejected"
        );
    }
    assert!(cat.list_albums().unwrap().is_empty());
}

#[test]
fn update_album_changes_the_definition_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let album = cat.create_album("Old", "2024-01-01", None, None).unwrap();
    cat.add_to_album(album.id, &[id]).unwrap();

    let updated =
        cat.update_album(album.id, "New name", "2024-02-03", Some("Rome"), Some("notes")).unwrap();

    assert_eq!(updated.name, "New name");
    assert_eq!(updated.date.as_deref(), Some("2024-02-03"));
    assert_eq!(updated.place.as_deref(), Some("Rome"));
    assert_eq!(updated.count, 1);
    assert_eq!(updated.uuid, album.uuid);
    assert_eq!(sidecar_albums(&original), vec![album.uuid]);
}

#[test]
fn add_to_album_writes_fb_albums_to_the_sidecar_and_the_mirror() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let album = cat.create_album("Trip", "2024-05-05", None, None).unwrap();

    assert_eq!(cat.add_to_album(album.id, &[id]).unwrap(), 1);

    assert_eq!(sidecar_albums(&original), vec![album.uuid.clone()]);
    let raw = std::fs::read_to_string(sidecar_path(original.to_str().unwrap())).unwrap();
    assert!(raw.contains("<fb:Albums>"), "{raw}");
    assert_eq!(cat.album_file_ids(album.id).unwrap(), vec![id]);
    assert_eq!(cat.file_album_ids(id).unwrap(), vec![album.id]);
    assert_eq!(cat.list_albums().unwrap()[0].count, 1);
}

#[test]
fn adding_twice_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let album = cat.create_album("Trip", "2024-05-05", None, None).unwrap();

    assert_eq!(cat.add_to_album(album.id, &[id]).unwrap(), 1);
    assert_eq!(cat.add_to_album(album.id, &[id]).unwrap(), 0);

    assert_eq!(sidecar_albums(&original).len(), 1);
    assert_eq!(cat.list_albums().unwrap()[0].count, 1);
}

#[test]
fn a_photo_can_be_in_many_albums_without_copying() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let a = cat.create_album("A", "2024-01-01", None, None).unwrap();
    let b = cat.create_album("B", "2024-01-02", None, None).unwrap();

    cat.add_to_album(a.id, &[id]).unwrap();
    cat.add_to_album(b.id, &[id]).unwrap();

    let mut ids = sidecar_albums(&original);
    ids.sort();
    let mut want = vec![a.uuid, b.uuid];
    want.sort();
    assert_eq!(ids, want);
    assert_eq!(cat.file_album_ids(id).unwrap().len(), 2);
    let files: i64 = cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap();
    assert_eq!(files, 1, "membership must not copy photos");
}

#[test]
fn adding_to_an_unknown_album_or_file_fails_without_touching_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let album = cat.create_album("A", "2024-01-01", None, None).unwrap();

    assert!(matches!(cat.add_to_album(999, &[id]), Err(SyncError::UnknownAlbum(999))));
    assert!(matches!(cat.add_to_album(album.id, &[12345]), Err(SyncError::UnknownFile(12345))));
    assert!(!sidecar_path(original.to_str().unwrap()).exists());
}

#[test]
fn remove_from_album_removes_the_fragment_and_keeps_other_albums() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let a = cat.create_album("A", "2024-01-01", None, None).unwrap();
    let b = cat.create_album("B", "2024-01-02", None, None).unwrap();
    cat.add_to_album(a.id, &[id]).unwrap();
    cat.add_to_album(b.id, &[id]).unwrap();

    assert_eq!(cat.remove_from_album(a.id, &[id]).unwrap(), 1);
    assert_eq!(sidecar_albums(&original), vec![b.uuid.clone()]);
    assert!(cat.album_file_ids(a.id).unwrap().is_empty());

    cat.remove_from_album(b.id, &[id]).unwrap();
    let raw = std::fs::read_to_string(sidecar_path(original.to_str().unwrap())).unwrap();
    assert!(!raw.contains("fb:Albums"), "{raw}");
    assert_eq!(cat.remove_from_album(b.id, &[id]).unwrap(), 0);
}

#[test]
fn membership_writes_keep_the_rest_of_the_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.set_rating(id, 5).unwrap();
    cat.add_tag(id, "sea").unwrap();
    cat.set_caption(id, "waves").unwrap();
    let album = cat.create_album("A", "2024-01-01", None, None).unwrap();

    cat.add_to_album(album.id, &[id]).unwrap();
    cat.remove_from_album(album.id, &[id]).unwrap();

    let sc = Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap();
    assert_eq!(sc.rating(), Some(5));
    assert_eq!(sc.tags(), vec!["sea"]);
    assert_eq!(sc.caption().as_deref(), Some("waves"));
}

#[test]
fn delete_album_leaves_photos_and_originals_intact() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id1, original1) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (id2, original2) = add_photo(&mut cat, dir.path(), "b.jpg");
    let keep = cat.create_album("Keep", "2024-01-01", None, None).unwrap();
    let doomed = cat.create_album("Doomed", "2024-01-02", None, None).unwrap();
    cat.add_to_album(doomed.id, &[id1, id2]).unwrap();
    cat.add_to_album(keep.id, &[id1]).unwrap();
    let before = (std::fs::read(&original1).unwrap(), std::fs::read(&original2).unwrap());

    cat.delete_album(doomed.id).unwrap();

    let albums = cat.list_albums().unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].id, keep.id);
    assert_eq!(sidecar_albums(&original1), vec![keep.uuid.clone()]);
    assert!(sidecar_albums(&original2).is_empty());
    let files: i64 = cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap();
    assert_eq!(files, 2);
    assert_eq!((std::fs::read(&original1).unwrap(), std::fs::read(&original2).unwrap()), before);
}

#[test]
fn deleted_albums_do_not_come_back_after_a_catalog_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    let album = cat.create_album("Gone", "2024-01-01", None, None).unwrap();
    cat.add_to_album(album.id, &[id]).unwrap();
    cat.delete_album(album.id).unwrap();
    drop(cat);

    delete_catalog(dir.path());
    let mut cat = open(dir.path());
    add_photo(&mut cat, dir.path(), "a.jpg");
    cat.rebuild_from_sidecars(true).unwrap();

    assert!(cat.list_albums().unwrap().is_empty());
}

#[test]
fn albums_and_membership_survive_deleting_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id1, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (id2, _) = add_photo(&mut cat, dir.path(), "b.jpg");
    let (_, _) = add_photo(&mut cat, dir.path(), "c.jpg");
    let album =
        cat.create_album("Summer 2024", "2024-07-14", Some("Lisbon"), Some("Two weeks")).unwrap();
    cat.add_to_album(album.id, &[id1, id2]).unwrap();
    drop(cat);

    delete_catalog(dir.path());
    let mut cat = open(dir.path());
    let (r1, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (r2, _) = add_photo(&mut cat, dir.path(), "b.jpg");
    add_photo(&mut cat, dir.path(), "c.jpg");
    cat.rebuild_from_sidecars(true).unwrap();

    let albums = cat.list_albums().unwrap();
    assert_eq!(albums.len(), 1);
    let restored = &albums[0];
    assert_eq!(restored.uuid, album.uuid);
    assert_eq!(restored.name, "Summer 2024");
    assert_eq!(restored.date.as_deref(), Some("2024-07-14"));
    assert_eq!(restored.place.as_deref(), Some("Lisbon"));
    assert_eq!(restored.description.as_deref(), Some("Two weeks"));
    assert_eq!(restored.count, 2);
    let mut members = cat.album_file_ids(restored.id).unwrap();
    members.sort();
    assert_eq!(members, vec![r1, r2]);
}

#[test]
fn an_incremental_rescan_after_catalog_deletion_restores_membership_too() {
    // The scan hook (`sync_paths`) is what runs after a re-scan.
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let album = cat.create_album("Trip", "2024-05-05", None, None).unwrap();
    cat.add_to_album(album.id, &[id]).unwrap();
    drop(cat);

    delete_catalog(dir.path());
    let mut cat = open(dir.path());
    let (rid, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.sync_paths([original.to_str().unwrap()]);

    let albums = cat.list_albums().unwrap();
    assert_eq!(albums[0].name, "Trip");
    assert_eq!(cat.album_file_ids(albums[0].id).unwrap(), vec![rid]);
}

#[test]
fn an_album_id_without_a_definition_is_recovered_as_a_placeholder() {
    // albums.json lost, sidecars intact: membership must not be dropped.
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (_, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let sc_path = sidecar_path(original.to_str().unwrap());
    let mut sc = Sidecar::read(&sc_path).unwrap();
    sc.set_albums(vec!["7d3c6a52-5a0e-4b3e-9a55-0d6a9f0f1a11".to_string()]);
    sc.write(&sc_path).unwrap();

    cat.rebuild_from_sidecars(true).unwrap();

    let albums = cat.list_albums().unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].uuid, "7d3c6a52-5a0e-4b3e-9a55-0d6a9f0f1a11");
    assert_eq!(albums[0].count, 1);
    assert!(!albums[0].name.is_empty());
}

#[test]
fn sidecar_wins_when_membership_changed_outside_fotobase() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let album = cat.create_album("Trip", "2024-05-05", None, None).unwrap();
    cat.add_to_album(album.id, &[id]).unwrap();

    let sc_path = sidecar_path(original.to_str().unwrap());
    let mut sc = Sidecar::read(&sc_path).unwrap();
    sc.set_albums(Vec::new());
    sc.write(&sc_path).unwrap();
    cat.rebuild_from_sidecars(true).unwrap();

    assert!(cat.album_file_ids(album.id).unwrap().is_empty());
}

#[test]
fn definitions_file_lives_next_to_the_catalog_and_keeps_unknown_fields() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let album = cat.create_album("Trip", "2024-05-05", None, None).unwrap();
    let json_path = dir.path().join("albums.json");
    assert_eq!(cat.albums_json_path(), json_path);

    let mut doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json_path).unwrap()).unwrap();
    assert_eq!(doc["version"], 1);
    assert_eq!(doc["albums"][0]["id"], album.uuid);
    assert_eq!(doc["albums"][0]["name"], "Trip");
    doc["future_root"] = serde_json::json!({"keep": true});
    doc["albums"][0]["future_field"] = serde_json::json!(42);
    std::fs::write(&json_path, serde_json::to_string(&doc).unwrap()).unwrap();

    cat.update_album(album.id, "Trip 2", "2024-05-06", None, None).unwrap();

    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json_path).unwrap()).unwrap();
    assert_eq!(doc["albums"][0]["name"], "Trip 2");
    assert_eq!(doc["albums"][0]["future_field"], 42);
    assert_eq!(doc["future_root"]["keep"], true);
    assert!(dir.path().join("albums.json.fb-bak").exists(), "previous version is backed up");
    assert!(!dir.path().join("albums.json.tmp").exists());
}

#[test]
fn cover_defaults_to_the_first_added_photo_and_can_be_picked() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id1, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (id2, _) = add_photo(&mut cat, dir.path(), "b.jpg");
    let album = cat.create_album("Trip", "2024-05-05", None, None).unwrap();
    assert_eq!(cat.list_albums().unwrap()[0].cover_file_id, None);

    cat.add_to_album(album.id, &[id2]).unwrap();
    cat.add_to_album(album.id, &[id1]).unwrap();
    assert_eq!(cat.list_albums().unwrap()[0].cover_file_id, Some(id2), "first added");

    cat.set_album_cover(album.id, id1).unwrap();
    assert_eq!(cat.list_albums().unwrap()[0].cover_file_id, Some(id1), "user-picked");

    // A picked cover that left the album falls back to the first member.
    cat.remove_from_album(album.id, &[id1]).unwrap();
    assert_eq!(cat.list_albums().unwrap()[0].cover_file_id, Some(id2));
}

#[test]
fn only_members_can_be_the_cover() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    let album = cat.create_album("Trip", "2024-05-05", None, None).unwrap();

    assert!(matches!(cat.set_album_cover(album.id, id), Err(SyncError::NotInAlbum { .. })));
}

#[test]
fn a_picked_cover_survives_a_catalog_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id1, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (id2, _) = add_photo(&mut cat, dir.path(), "b.jpg");
    let album = cat.create_album("Trip", "2024-05-05", None, None).unwrap();
    cat.add_to_album(album.id, &[id1, id2]).unwrap();
    cat.set_album_cover(album.id, id2).unwrap();
    drop(cat);

    delete_catalog(dir.path());
    let mut cat = open(dir.path());
    add_photo(&mut cat, dir.path(), "a.jpg");
    let (r2, _) = add_photo(&mut cat, dir.path(), "b.jpg");
    cat.rebuild_from_sidecars(true).unwrap();

    assert_eq!(cat.list_albums().unwrap()[0].cover_file_id, Some(r2));
}

#[test]
fn album_files_lists_members_across_folders() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id1, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (id2, _) = add_photo(&mut cat, other.path(), "b.jpg");
    let (_, _) = add_photo(&mut cat, dir.path(), "c.jpg");
    let album = cat.create_album("Mixed", "2024-05-05", None, None).unwrap();
    cat.add_to_album(album.id, &[id1, id2]).unwrap();

    let files = cat.list_album_files(album.id, usize::MAX).unwrap();

    assert_eq!(files.iter().map(|f| f.id).collect::<Vec<_>>(), vec![id1, id2]);
    assert_eq!(cat.list_albums().unwrap()[0].count, 2);
}

#[test]
fn originals_are_never_touched_by_album_operations() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let before = std::fs::read(&original).unwrap();
    let mtime = std::fs::metadata(&original).unwrap().modified().unwrap();

    let album = cat.create_album("A", "2024-01-01", None, None).unwrap();
    cat.add_to_album(album.id, &[id]).unwrap();
    cat.set_album_cover(album.id, id).unwrap();
    cat.remove_from_album(album.id, &[id]).unwrap();
    cat.delete_album(album.id).unwrap();

    assert_eq!(std::fs::read(&original).unwrap(), before);
    assert_eq!(std::fs::metadata(&original).unwrap().modified().unwrap(), mtime);
}

#[test]
fn a_failed_sidecar_write_leaves_membership_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let album = cat.create_album("A", "2024-01-01", None, None).unwrap();
    // A malformed sidecar makes the read-modify-write fail loudly.
    std::fs::write(sidecar_path(original.to_str().unwrap()), "<not-xmp").unwrap();

    assert!(cat.add_to_album(album.id, &[id]).is_err());
    assert!(cat.album_file_ids(album.id).unwrap().is_empty());
}

#[test]
fn search_inside_an_album_only_matches_its_members() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let (id1, _) = add_photo(&mut cat, dir.path(), "beach_a.jpg");
    let (id2, _) = add_photo(&mut cat, dir.path(), "city_b.jpg");
    let (_outside, _) = add_photo(&mut cat, dir.path(), "beach_c.jpg");
    cat.set_caption(id2, "beach at night").unwrap();
    let album = cat.create_album("Mixed", "2024-05-05", None, None).unwrap();
    cat.add_to_album(album.id, &[id1, id2]).unwrap();

    let hits = cat.search_album_files(album.id, "beach", 10).unwrap();

    let mut ids: Vec<i64> = hits.iter().map(|f| f.id).collect();
    ids.sort();
    assert_eq!(ids, vec![id1, id2], "the non-member beach photo must not match");
    assert!(cat.search_album_files(album.id, "   ", 10).unwrap().is_empty());
}
