//! Lazy content-hash backfill (PLAN §10 "hash lazily"): the catalog side.
//!
//! The scanner inserts `hash IS NULL` rows (it never reads file contents);
//! something must later fill them so the grid can build `thumb://` URIs.

use fotobase_catalog::Catalog;
use tempfile::TempDir;

fn open_in(dir: &TempDir) -> Catalog {
    Catalog::open(dir.path().join("catalog.db")).expect("open catalog")
}

fn seed_folder(cat: &Catalog, path: &str) {
    cat.conn().execute("INSERT INTO folders(path, scan_mode) VALUES (?1, 'once')", [path]).unwrap();
}

fn seed_file(cat: &Catalog, folder_id: i64, path: &str, hash: Option<&str>) -> i64 {
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, hash, media_type) \
             VALUES (?1, ?2, 'a.jpg', 10, 100, ?3, 'image')",
            rusqlite::params![folder_id, path, hash],
        )
        .unwrap();
    cat.conn().last_insert_rowid()
}

#[test]
fn unhashed_files_lists_only_null_hash_rows_path_ordered() {
    let dir = TempDir::new().unwrap();
    let cat = open_in(&dir);
    seed_folder(&cat, "/lib");
    let folder = cat.conn().query_row("SELECT id FROM folders", [], |r| r.get(0)).unwrap();

    let hashed = seed_file(&cat, folder, "/lib/b.jpg", Some("ab".repeat(16).as_str()));
    let _b = seed_file(&cat, folder, "/lib/a.jpg", None);
    let _c = seed_file(&cat, folder, "/lib/c.jpg", None);

    let rows = cat.unhashed_files(usize::MAX).unwrap();
    assert_eq!(
        rows,
        vec![(2, "/lib/a.jpg".to_string()), (3, "/lib/c.jpg".to_string())],
        "only NULL-hash rows, path-ordered"
    );

    // The limit is honored (the backfill works in batches).
    let one = cat.unhashed_files(1).unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].1, "/lib/a.jpg");

    // The already-hashed row is never listed.
    assert!(rows.iter().all(|(id, _)| *id != hashed));
}

#[test]
fn set_file_hash_persists_and_clears_the_unhashed_list() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    seed_folder(&cat, "/lib");
    let folder = cat.conn().query_row("SELECT id FROM folders", [], |r| r.get(0)).unwrap();
    let id = seed_file(&cat, folder, "/lib/a.jpg", None);

    cat.set_file_hash(id, "cd".repeat(16).as_str()).unwrap();

    let stored: Option<String> =
        cat.conn().query_row("SELECT hash FROM files WHERE id = ?1", [id], |r| r.get(0)).unwrap();
    assert_eq!(stored.as_deref(), Some(&"cd".repeat(16)[..]));
    assert!(cat.unhashed_files(usize::MAX).unwrap().is_empty(), "row no longer unhashed");
}

#[test]
fn unknown_or_deleted_rows_are_quiet_no_ops() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    // No rows at all: both queries are empty, never errors.
    assert!(cat.unhashed_files(usize::MAX).unwrap().is_empty());
    cat.set_file_hash(12345, "ab".repeat(16).as_str()).unwrap();
}
