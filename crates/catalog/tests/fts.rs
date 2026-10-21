//! FTS5 live search tests (TASK-01-008, PLAN §5 `files_fts`).
//!
//! Search covers filename + folder, sync via triggers, prefix matching for
//! live as-you-type queries.

use fotobase_catalog::Catalog;
use tempfile::TempDir;

fn open_in(dir: &TempDir) -> Catalog {
    Catalog::open(dir.path().join("catalog.db")).expect("open catalog")
}

/// Insert a folder + file row the way the scanner does (direct SQL; the FTS
/// sync must work without scanner involvement).
fn add_file(cat: &mut Catalog, folder: &str, filename: &str) -> i64 {
    let folder_id: i64 = cat.seed_folder(folder).expect("seed folder");
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, ?2, ?3, 1, 0.0, 'image')",
            rusqlite::params![folder_id, format!("{folder}/{filename}"), filename],
        )
        .expect("insert file");
    cat.conn().last_insert_rowid()
}

#[test]
fn search_matches_filename_and_folder() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let id = add_file(&mut cat, "/photos/2024", "beach.jpg");

    assert_eq!(cat.search_files("beach", 100).unwrap(), vec![id]);
    assert_eq!(cat.search_files("2024", 100).unwrap(), vec![id]);
    assert_eq!(cat.search_files("xyzzy", 100).unwrap(), vec![]);
}

#[test]
fn search_is_case_insensitive_and_prefix_based() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let id = add_file(&mut cat, "/photos/2024", "BeachSunset.jpg");

    // Live search as you type: partial input already matches.
    assert_eq!(cat.search_files("beach", 100).unwrap(), vec![id]);
    assert_eq!(cat.search_files("BEACHSUN", 100).unwrap(), vec![id]);
}

#[test]
fn multiple_tokens_require_all_matches() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let beach = add_file(&mut cat, "/photos/2024", "beach.jpg");
    add_file(&mut cat, "/photos/2024", "sunset.jpg");

    assert_eq!(cat.search_files("beach", 100).unwrap(), vec![beach]);
    assert_eq!(cat.search_files("2024 beach", 100).unwrap(), vec![beach]);
}

#[test]
fn search_survives_fts_syntax_characters_in_input() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let id = add_file(&mut cat, "/photos/2024", "beach (1).jpg");

    // Typed text must never be interpreted as FTS5 query syntax.
    assert_eq!(cat.search_files("beach (", 100).unwrap(), vec![id]);
    assert_eq!(cat.search_files("\"", 100).unwrap(), vec![]);
}

#[test]
fn blank_query_returns_no_results() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    add_file(&mut cat, "/photos", "a.jpg");
    assert!(cat.search_files("", 100).unwrap().is_empty());
    assert!(cat.search_files("   ", 100).unwrap().is_empty());
}

#[test]
fn fts_stays_in_sync_on_rename_and_delete() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let id = add_file(&mut cat, "/photos", "beach.jpg");

    // Rename: new name searchable, old name gone.
    cat.conn().execute("UPDATE files SET filename = 'sunset.jpg' WHERE id = ?1", [id]).unwrap();
    assert_eq!(cat.search_files("sunset", 100).unwrap(), vec![id]);
    assert!(cat.search_files("beach", 100).unwrap().is_empty());

    // Delete: no ghost hits.
    cat.conn().execute("DELETE FROM files WHERE id = ?1", [id]).unwrap();
    assert!(cat.search_files("sunset", 100).unwrap().is_empty());
    assert!(cat.search_files("photos", 100).unwrap().is_empty());
}

#[test]
fn limit_caps_result_count() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    for i in 0..5 {
        add_file(&mut cat, "/photos", &format!("beach{i}.jpg"));
    }
    assert_eq!(cat.search_files("beach", 3).unwrap().len(), 3);
}

#[test]
fn fts_backfills_existing_rows_on_migration() {
    // A v1 catalog (pre-01-008) already holds files; migration v2 must index
    // them, not only future inserts.
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("catalog.db");
    {
        let mut cat = Catalog::open(&path).unwrap();
        add_file(&mut cat, "/photos/2024", "beach.jpg");
        // Simulate the old build: strip triggers + wipe the index.
        cat.conn()
            .execute_batch(
                "DROP TRIGGER files_fts_insert;
             DROP TRIGGER files_fts_update;
             DROP TRIGGER files_fts_delete;
             DELETE FROM files_fts;
             PRAGMA user_version = 1;",
            )
            .unwrap();
    }
    let reopened = Catalog::open(&path).expect("reopen");
    assert_eq!(reopened.search_files("beach", 100).unwrap().len(), 1);
}
