//! TASK-01-017: duplicate detection at import time matches by content hash
//! against the `files` table. `has_hash` is the check the import engine
//! calls before copying a file; `NULL` hashes (not yet computed) never
//! match, exactly like `paths_for_hash`.

use fotobase_catalog::Catalog;

fn open_with_file(path: &str, hash: Option<&str>) -> Catalog {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("catalog.db")).unwrap();
    cat.seed_folder("/photos").unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, hash, media_type)
             VALUES (1, ?1, 'unused', 1, 0.0, ?2, 'image')",
            rusqlite::params![path, hash],
        )
        .unwrap();
    // Keep the connection valid for the rest of the test: leak the tempdir.
    std::mem::forget(dir);
    cat
}

#[test]
fn has_hash_matches_only_known_hashes() {
    let cat = open_with_file("/photos/a.jpg", Some("abc123"));
    assert!(cat.has_hash("abc123").unwrap());
    assert!(!cat.has_hash("other").unwrap(), "unknown hash → no duplicate");
}

#[test]
fn has_hash_ignores_unhashed_rows() {
    let cat = open_with_file("/photos/b.jpg", None);
    assert!(!cat.has_hash("").unwrap(), "empty/NULL hash never matches");
    assert!(!cat.has_hash("anything").unwrap());
}
