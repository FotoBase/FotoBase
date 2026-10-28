//! TASK-03-005: star/rating write-through, unstar, and the starred filter.

use std::path::{Path, PathBuf};

use fotobase_catalog::{sidecar_path, Catalog, SyncError};
use fotobase_metadata::Sidecar;

fn add_photo(cat: &mut Catalog, dir: &Path, name: &str) -> (i64, PathBuf) {
    let original = dir.join(name);
    std::fs::write(&original, b"fake image bytes").unwrap();
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

fn rating_of(cat: &Catalog, id: i64) -> u8 {
    cat.list_files(usize::MAX).unwrap().into_iter().find(|f| f.id == id).unwrap().rating
}

#[test]
fn star_writes_xmp_rating_5_and_catalog_follows() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");

    cat.set_rating(id, 5).unwrap();

    let sc = Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap();
    assert_eq!(sc.rating(), Some(5));
    assert_eq!(rating_of(&cat, id), 5);
}

#[test]
fn unstar_removes_the_rating_node_and_clears_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.set_rating(id, 5).unwrap();

    cat.clear_rating(id).unwrap();

    let sc_path = sidecar_path(original.to_str().unwrap());
    let sc = Sidecar::read(&sc_path).unwrap();
    assert_eq!(sc.rating(), None);
    assert!(!sc.raw().contains("xmp:Rating"), "node removed, not zeroed");
    assert_eq!(rating_of(&cat, id), 0);
}

#[test]
fn rating_above_5_is_rejected_before_touching_the_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");

    let err = cat.set_rating(id, 6).unwrap_err();
    assert!(matches!(err, SyncError::InvalidRating(6)), "{err:?}");
    assert!(!sidecar_path(original.to_str().unwrap()).exists());
}

#[test]
fn external_out_of_range_rating_does_not_break_convergence() {
    // Some tools write -1 (rejected) or junk; the catalog CHECK is 0..=5.
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let mut sc = Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap();
    sc.set_rating(9);
    sc.write(&sidecar_path(original.to_str().unwrap())).unwrap();

    cat.sync_file(id).expect("converges despite rating 9");
    assert_eq!(rating_of(&cat, id), 0);
}

#[test]
fn starred_filter_returns_only_rated_photos() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (a, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (b, _) = add_photo(&mut cat, dir.path(), "b.jpg");
    let (c, _) = add_photo(&mut cat, dir.path(), "c.jpg");
    cat.set_rating(a, 5).unwrap();
    cat.set_rating(b, 3).unwrap();
    let _ = c;

    // Star-only mode: star = 5.
    let starred: Vec<i64> =
        cat.list_starred(None, 5, usize::MAX).unwrap().into_iter().map(|f| f.id).collect();
    assert_eq!(starred, vec![a]);

    // Ratings mode: any 1-5 counts.
    let rated: Vec<i64> =
        cat.list_starred(None, 1, usize::MAX).unwrap().into_iter().map(|f| f.id).collect();
    assert_eq!(rated, vec![a, b]);
}

#[test]
fn starred_filter_honours_folder_selection() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (a, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (b, _) = add_photo(&mut cat, &sub, "b.jpg");
    cat.set_rating(a, 5).unwrap();
    cat.set_rating(b, 5).unwrap();

    let folder_a: i64 = cat
        .conn()
        .query_row("SELECT folder_id FROM files WHERE id = ?1", [a], |r| r.get(0))
        .unwrap();
    let only_a = cat.list_starred(Some(&[folder_a]), 5, usize::MAX).unwrap();
    assert_eq!(only_a.iter().map(|f| f.id).collect::<Vec<_>>(), vec![a]);
    assert!(cat.list_starred(Some(&[]), 5, usize::MAX).unwrap().is_empty());
}

#[test]
fn photo_with_xmp_rating_5_in_its_sidecar_is_starred_after_sync() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let sc_path = sidecar_path(original.to_str().unwrap());
    let mut sc = Sidecar::read(&sc_path).unwrap();
    sc.set_rating(5);
    sc.write(&sc_path).unwrap();

    cat.sync_file(id).unwrap();

    let starred = cat.list_starred(None, 5, usize::MAX).unwrap();
    assert_eq!(starred.len(), 1);
    assert_eq!(starred[0].id, id);
    assert_eq!(starred[0].rating, 5);
}
