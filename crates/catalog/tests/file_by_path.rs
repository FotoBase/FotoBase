//! `file_id_by_path` (TASK-01-025): routing an OS-opened file into the
//! viewer needs a path → catalog-row lookup. `files.path` is UNIQUE, so the
//! lookup is a single indexed query.

use fotobase_catalog::Catalog;

fn open_with_file(path: &str) -> Catalog {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("catalog.db")).unwrap();
    cat.seed_folder("/photos").unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, media_type)
             VALUES (1, ?1, 'a.jpg', 1, 0.0, 'image')",
            rusqlite::params![path],
        )
        .unwrap();
    // Keep the connection valid for the rest of the test: leak the tempdir.
    std::mem::forget(dir);
    cat
}

#[test]
fn file_id_by_path_finds_the_row() {
    let cat = open_with_file("/photos/a.jpg");
    assert_eq!(cat.file_id_by_path("/photos/a.jpg").unwrap(), Some(1));
}

#[test]
fn file_id_by_path_misses_are_none() {
    let cat = open_with_file("/photos/a.jpg");
    assert_eq!(cat.file_id_by_path("/photos/b.jpg").unwrap(), None);
}
