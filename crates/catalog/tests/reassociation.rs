//! TASK-03-003: missing-file records for move/rename re-association
//! (00-010 design §4). When the scanner deletes rows for vanished files it
//! first snapshots them into `missing_files`; a later scan matches new-looking
//! paths against that set by (size, mtime window, lazy hash).

use fotobase_catalog::Catalog;
use tempfile::TempDir;

fn open_in(dir: &TempDir) -> Catalog {
    Catalog::open(dir.path().join("catalog.db")).expect("open catalog")
}

fn seed_file(
    cat: &mut Catalog,
    folder: &str,
    path: &str,
    size: i64,
    mtime: f64,
    hash: Option<&str>,
) -> i64 {
    cat.seed_folder(folder).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, hash, media_type)
             VALUES ((SELECT id FROM folders WHERE path = ?1), ?2, 'a.jpg', ?3, ?4, ?5, 'image')",
            rusqlite::params![folder, path, size, mtime, hash],
        )
        .unwrap();
    cat.conn().last_insert_rowid()
}

fn user_data(cat: &Catalog, file_id: i64) -> (Option<i64>, usize) {
    let rating: Option<i64> = cat
        .conn()
        .query_row("SELECT rating FROM metadata WHERE file_id = ?1", [file_id], |r| r.get(0))
        .unwrap();
    let tags: i64 = cat
        .conn()
        .query_row("SELECT COUNT(*) FROM file_tags WHERE file_id = ?1", [file_id], |r| r.get(0))
        .unwrap();
    (rating, tags as usize)
}

#[test]
fn snapshot_missing_copies_vanished_rows_including_hash() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let id = seed_file(&mut cat, "/lib", "/lib/a.jpg", 123, 100.0, Some("ab".repeat(16).as_str()));

    cat.snapshot_missing(&["/lib/a.jpg".to_string()]).unwrap();

    let rec = &cat.missing_with_size(123, "image").unwrap()[0];
    assert_eq!(rec.path, "/lib/a.jpg");
    assert_eq!(rec.size, 123);
    assert_eq!(rec.mtime, 100.0);
    assert_eq!(rec.hash.as_deref(), Some("ab".repeat(16).as_str()));

    // The original row is still there — snapshot does not delete.
    assert!(cat.remove_file(id).unwrap());
}

#[test]
fn reassociate_restores_user_data_on_the_new_row() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    cat.seed_folder("/new").unwrap();
    let id = seed_file(&mut cat, "/lib", "/lib/a.jpg", 123, 100.0, Some("ab".repeat(16).as_str()));
    cat.conn().execute("INSERT INTO metadata (file_id, rating) VALUES (?1, 4)", [id]).unwrap();
    cat.conn().execute("INSERT INTO tags (name) VALUES ('family')", []).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO file_tags (file_id, tag_id) VALUES (?1, (SELECT id FROM tags WHERE name = 'family'))",
            [id],
        )
        .unwrap();
    cat.snapshot_missing(&["/lib/a.jpg".to_string()]).unwrap();
    cat.remove_file(id).unwrap();

    let rec = cat.missing_with_size(123, "image").unwrap().remove(0);
    let file_id = cat.reassociate_file(rec.id, "/new/b.jpg", "/new", 123, 200.0).unwrap();
    assert!(file_id.is_some());

    let (path, rating, tags): (String, Option<i64>, i64) = cat
        .conn()
        .query_row(
            "SELECT f.path, m.rating, (SELECT COUNT(*) FROM file_tags WHERE file_id = f.id)
             FROM files f LEFT JOIN metadata m ON m.file_id = f.id WHERE f.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(path, "/new/b.jpg");
    assert_eq!(rating, Some(4));
    assert_eq!(tags, 1);
    let (rating, tags) = user_data(&cat, file_id.unwrap());
    assert_eq!(rating, Some(4));
    assert_eq!(tags, 1);

    // The missing record is consumed.
    assert!(cat.missing_with_size(123, "image").unwrap().is_empty());
}

#[test]
fn reappearing_path_drops_its_missing_record() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    seed_file(&mut cat, "/lib", "/lib/a.jpg", 123, 100.0, None);
    cat.snapshot_missing(&["/lib/a.jpg".to_string()]).unwrap();
    assert_eq!(cat.missing_with_size(123, "image").unwrap().len(), 1);

    cat.drop_missing("/lib/a.jpg").unwrap();
    assert!(cat.missing_with_size(123, "image").unwrap().is_empty());
}

#[test]
fn missing_candidates_filter_by_media_type() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    seed_file(&mut cat, "/lib", "/lib/a.jpg", 123, 100.0, None);
    cat.snapshot_missing(&["/lib/a.jpg".to_string()]).unwrap();

    assert!(cat.missing_with_size(123, "video").unwrap().is_empty());
    assert_eq!(cat.missing_with_size(123, "image").unwrap().len(), 1);
    assert!(cat.missing_with_size(999, "image").unwrap().is_empty());
}
