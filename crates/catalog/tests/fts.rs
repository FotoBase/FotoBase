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

// --- TASK-01-018: live search bar. The grid needs rows (filename + hash),
// not bare ids, and the query must combine with the folder-tree selection.

/// Insert a file into an existing folder row (folder id already known).
fn add_file_in(
    cat: &mut Catalog,
    folder_id: i64,
    folder: &str,
    filename: &str,
    hash: Option<&str>,
) {
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, hash, size, mtime, media_type)
             VALUES (?1, ?2, ?3, ?4, 1, 0.0, 'image')",
            rusqlite::params![folder_id, format!("{folder}/{filename}"), filename, hash],
        )
        .expect("insert file");
}

#[test]
fn detailed_search_returns_grid_rows() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let folder_id = cat.seed_folder("/photos/2024").unwrap();
    add_file_in(&mut cat, folder_id, "/photos/2024", "beach.jpg", Some("aabb"));

    let rows = cat.search_files_detailed("beach", None, 100).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].filename, "beach.jpg");
    assert_eq!(rows[0].hash.as_deref(), Some("aabb"));

    // Folder names match too (01-008 semantics carried over).
    assert_eq!(cat.search_files_detailed("2024", None, 100).unwrap().len(), 1);

    // Empty/whitespace queries match nothing (the UI routes those to the
    // plain list, the API must never leak the whole table here).
    assert!(cat.search_files_detailed("", None, 100).unwrap().is_empty());
    assert!(cat.search_files_detailed("   ", None, 100).unwrap().is_empty());
}

#[test]
fn detailed_search_intersects_with_folder_selection() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let f2024 = cat.seed_folder("/photos/2024").unwrap();
    let f2023 = cat.seed_folder("/photos/2023").unwrap();
    add_file_in(&mut cat, f2024, "/photos/2024", "beach.jpg", None);
    add_file_in(&mut cat, f2023, "/photos/2023", "beach.png", None);

    // Query + folder selection: only the intersection survives.
    let ids: Vec<i64> = cat
        .search_files_detailed("beach", Some(&[f2024]), 100)
        .unwrap()
        .iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(ids.len(), 1);
    assert_eq!(
        ids[0],
        cat.conn()
            .query_row("SELECT id FROM files WHERE filename = 'beach.jpg'", [], |r| r.get(0))
            .unwrap()
    );

    assert_eq!(cat.search_files_detailed("beach", Some(&[f2023]), 100).unwrap().len(), 1);
    assert_eq!(cat.search_files_detailed("beach", Some(&[f2024, f2023]), 100).unwrap().len(), 2);

    // Empty selection filters everything out (list_files_in_folders parity).
    assert!(cat.search_files_detailed("beach", Some(&[]), 100).unwrap().is_empty());

    // A folder-matching query stays restricted to the selection.
    assert!(cat.search_files_detailed("2023", Some(&[f2024]), 100).unwrap().is_empty());
}

#[test]
fn fts_backfills_existing_rows_on_migration() {
    // A genuine v1 catalog (pre-01-008) already holds files; migration v2
    // must index them, not only future inserts. Build the v1 schema from the
    // recorded migration SQL, insert via plain SQL, then let `Catalog::open`
    // run migrations v2+.
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("catalog.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(fotobase_catalog::MIGRATIONS[0]).unwrap();
        conn.execute_batch("PRAGMA user_version = 1").unwrap();
        conn.execute("INSERT INTO folders(path, scan_mode) VALUES ('/photos/2024', 'once')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, media_type)
             VALUES (1, '/photos/2024/beach.jpg', 'beach.jpg', 1, 0.0, 'image')",
            [],
        )
        .unwrap();
    }
    let reopened = Catalog::open(&path).expect("reopen");
    assert_eq!(reopened.search_files("beach", 100).unwrap().len(), 1);
}
