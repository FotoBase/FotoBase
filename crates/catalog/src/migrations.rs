//! Forward-only schema migrations for the SQLite catalog (PLAN §5).
//!
//! The catalog is a rebuildable cache, but upgrades must never lose data:
//! migrations run in order, each inside a transaction, tracked by
//! `PRAGMA user_version`. A database created by a *newer* version of the app is
//! rejected (`CatalogError::NewerSchema`) — we never downgrade.

/// Ordered migration SQL. Index 0 upgrades a version-0 database to version 1.
pub const MIGRATIONS: &[&str] = &[MIGRATION_V1, MIGRATION_V2];

const MIGRATION_V1: &str = "
CREATE TABLE folders (
    id          INTEGER PRIMARY KEY,
    path        TEXT NOT NULL UNIQUE,
    scan_mode   TEXT NOT NULL DEFAULT 'once' CHECK (scan_mode IN ('once', 'always')),
    last_scan_at TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE files (
    id         INTEGER PRIMARY KEY,
    folder_id  INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    path       TEXT NOT NULL UNIQUE,
    filename   TEXT NOT NULL,
    size       INTEGER NOT NULL,
    mtime      REAL NOT NULL,
    hash       TEXT,
    width      INTEGER,
    height     INTEGER,
    taken_at   TEXT,
    media_type TEXT NOT NULL CHECK (media_type IN ('image', 'video'))
);
CREATE INDEX idx_files_folder ON files(folder_id);
CREATE INDEX idx_files_hash ON files(hash);
CREATE INDEX idx_files_mtime ON files(mtime);

CREATE TABLE metadata (
    file_id      INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    camera       TEXT,
    lens         TEXT,
    exposure     TEXT,
    f_number     REAL,
    iso          INTEGER,
    focal_length REAL,
    caption      TEXT,
    rating       INTEGER CHECK (rating BETWEEN 0 AND 5),
    latitude     REAL,
    longitude    REAL,
    orientation  INTEGER
);

CREATE TABLE tags (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE file_tags (
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (file_id, tag_id)
);

CREATE TABLE albums (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    date        TEXT,
    place       TEXT,
    description TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE album_files (
    album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    file_id  INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (album_id, file_id)
);

CREATE VIRTUAL TABLE files_fts USING fts5(filename, folder);
";

/// TASK-01-008: keep `files_fts` in sync with `files` via triggers (rowid =
/// files.id), and backfill the index for rows that pre-date this migration.
const MIGRATION_V2: &str = "
CREATE TRIGGER files_fts_insert AFTER INSERT ON files BEGIN
    INSERT INTO files_fts(rowid, filename, folder)
    VALUES (NEW.id, NEW.filename, (SELECT path FROM folders WHERE id = NEW.folder_id));
END;

CREATE TRIGGER files_fts_delete AFTER DELETE ON files BEGIN
    DELETE FROM files_fts WHERE rowid = OLD.id;
END;

CREATE TRIGGER files_fts_update AFTER UPDATE ON files BEGIN
    DELETE FROM files_fts WHERE rowid = OLD.id;
    INSERT INTO files_fts(rowid, filename, folder)
    VALUES (NEW.id, NEW.filename, (SELECT path FROM folders WHERE id = NEW.folder_id));
END;

INSERT INTO files_fts(rowid, filename, folder)
    SELECT f.id, f.filename, d.path FROM files f JOIN folders d ON d.id = f.folder_id;
";

use rusqlite::Connection;

/// Apply all pending migrations, in order, each in its own transaction.
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let target = (i + 1) as i64;
        if version >= target {
            continue;
        }
        tracing::info!(target, "applying catalog migration");
        conn.execute_batch(&format!("BEGIN; {sql} PRAGMA user_version = {target}; COMMIT;"))?;
    }
    Ok(())
}
