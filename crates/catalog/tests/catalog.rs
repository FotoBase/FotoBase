//! Integration tests for the catalog crate (TASK-01-005, PLAN §5 "SQLite robustness").

use fotobase_catalog::Catalog;
use rusqlite::Connection;
use tempfile::TempDir;

fn open_in(dir: &TempDir) -> Catalog {
    Catalog::open(dir.path().join("catalog.db")).expect("open catalog")
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
}

#[test]
fn migrations_run_in_order_on_an_empty_db() {
    let dir = TempDir::new().unwrap();
    let cat = open_in(&dir);

    // All migrations applied: user_version equals the number of migrations.
    assert_eq!(user_version(cat.conn()), fotobase_catalog::MIGRATIONS.len() as i64);

    // Schema matches the PLAN §5 table list (minus AI tables).
    let expected =
        ["folders", "files", "metadata", "tags", "file_tags", "albums", "album_files", "files_fts"];
    let mut stmt = cat
        .conn()
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap();
    let tables: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .filter(|n| !n.starts_with("sqlite_"))
        .collect();
    for name in expected {
        assert!(tables.iter().any(|t| t == name), "missing table {name} (have {tables:?})");
    }

    // files table carries the §5 columns.
    let mut stmt = cat.conn().prepare("PRAGMA table_info(files)").unwrap();
    let cols: Vec<String> =
        stmt.query_map([], |r| r.get::<_, String>(1)).unwrap().filter_map(|r| r.ok()).collect();
    for col in ["path", "size", "mtime", "hash", "width", "height", "taken_at", "media_type"] {
        assert!(cols.iter().any(|c| c == col), "files missing column {col}");
    }
}

#[test]
fn migrations_run_forward_only_on_a_previous_version_db() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("catalog.db");

    // Simulate a previous-version catalog: user_version = 0, no tables.
    {
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 0).unwrap();
    }

    let cat = Catalog::open(&path).expect("migrate previous-version db");
    assert_eq!(user_version(cat.conn()), fotobase_catalog::MIGRATIONS.len() as i64);

    // Reopening a migrated db is a no-op (idempotent, no downgrade).
    let reopened = Catalog::open(&path).expect("reopen migrated db");
    assert_eq!(user_version(reopened.conn()), fotobase_catalog::MIGRATIONS.len() as i64);
}

#[test]
fn opening_a_db_from_a_newer_version_fails_forward_only() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("catalog.db");
    {
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", fotobase_catalog::MIGRATIONS.len() as i64 + 1)
            .unwrap();
    }
    let err = Catalog::open(&path).expect_err("newer schema must be rejected");
    assert!(matches!(err, fotobase_catalog::CatalogError::NewerSchema { .. }), "got {err:?}");
}

#[test]
fn wal_mode_is_enabled() {
    let dir = TempDir::new().unwrap();
    let cat = open_in(&dir);
    let mode: String = cat.conn().query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
}

#[test]
fn integrity_check_passes_on_a_healthy_catalog() {
    let dir = TempDir::new().unwrap();
    let cat = open_in(&dir);
    cat.integrity_check().expect("healthy catalog passes integrity_check");
    cat.quick_check().expect("healthy catalog passes quick_check");
}

#[test]
fn integrity_check_detects_a_corrupted_catalog() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("catalog.db");
    {
        let _cat = Catalog::open(&path).expect("open");
    } // drop: flush WAL
    drop(Catalog::open(&path).expect("reopen to checkpoint"));

    // Corrupt the middle of the main db file.
    let mut bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() > 4096);
    for b in &mut bytes[4096..8192] {
        *b = 0xFF;
    }
    std::fs::write(&path, &bytes).unwrap();

    let err = Catalog::open(&path);
    match err {
        Ok(cat) => {
            // Open succeeded; the check itself must flag corruption.
            let e = cat.quick_check().expect_err("corruption must be detected");
            assert!(matches!(e, fotobase_catalog::CatalogError::Corrupted(_)), "got {e:?}");
        }
        Err(e) => assert!(matches!(e, fotobase_catalog::CatalogError::Corrupted(_)), "got {e:?}"),
    }
}

// TASK-01-011: the thumb:// scheme handler resolves a content hash back to
// source paths so a cache miss can trigger a render job.
#[test]
fn paths_for_hash_maps_content_hash_to_source_paths() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let folder = cat.seed_folder("/photos").unwrap();
    let insert = |path: &str, hash: Option<&str>| {
        cat.conn()
            .execute(
                "INSERT INTO files(folder_id, path, filename, size, mtime, hash, media_type)
                 VALUES (?1, ?2, ?3, 1, 0.0, ?4, 'image')",
                rusqlite::params![folder, path, "unused", hash],
            )
            .unwrap();
    };
    insert("/photos/a.jpg", Some("h1"));
    insert("/photos/b.jpg", Some("h1"));
    insert("/photos/c.jpg", None);

    let mut paths = cat.paths_for_hash("h1").unwrap();
    paths.sort();
    assert_eq!(paths, vec!["/photos/a.jpg", "/photos/b.jpg"]);
    assert!(cat.paths_for_hash("unknown").unwrap().is_empty(), "unknown hash → no paths");
    assert!(cat.paths_for_hash("h1").unwrap().len() == 2, "hash shared by duplicates");
}

#[test]
fn daily_backups_roll_and_keep_n() {
    let dir = TempDir::new().unwrap();
    let cat = open_in(&dir);

    // A backup taken today makes backup_if_stale a no-op until tomorrow.
    let first = cat.backup_if_stale(3).expect("stale backup").expect("first backup");
    assert!(first.exists());
    assert!(cat.backup_if_stale(3).expect("stale check").is_none(), "same day = no new backup");

    // Roll: create 5 dated backups, keep 3.
    for _ in 0..5 {
        cat.backup_now(3).expect("backup_now");
    }
    let mut backups: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("catalog.db.backup-"))
        .collect();
    backups.sort();
    assert_eq!(backups.len(), 3, "rolling backup keeps N ({backups:?})");
}

#[test]
fn backup_snapshot_is_a_valid_catalog() {
    let dir = TempDir::new().unwrap();
    let cat = open_in(&dir);
    cat.backup_now(2).expect("backup");
    let mut backups: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("catalog.db.backup-"))
        .collect();
    assert_eq!(backups.len(), 1);
    let backup = backups.pop().unwrap();
    let restored = Catalog::open(&backup).expect("backup is a valid catalog file");
    assert_eq!(user_version(restored.conn()), fotobase_catalog::MIGRATIONS.len() as i64);
}
