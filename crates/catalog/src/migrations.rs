//! Forward-only schema migrations for the SQLite catalog (PLAN §5).
//!
//! The catalog is a rebuildable cache, but upgrades must never lose data:
//! migrations run in order, each inside a transaction, tracked by
//! `PRAGMA user_version`. A database created by a *newer* version of the app is
//! rejected (`CatalogError::NewerSchema`) — we never downgrade.

/// Ordered migration SQL. Index 0 upgrades a version-0 database to version 1.
pub const MIGRATIONS: &[&str] = &[
    MIGRATION_V1,
    MIGRATION_V2,
    MIGRATION_V3,
    MIGRATION_V4,
    MIGRATION_V5,
    MIGRATION_V6,
    MIGRATION_V7,
    MIGRATION_V8,
    MIGRATION_V9,
    MIGRATION_V10,
];

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

/// TASK-03-002: sidecar sync state per file — the (mtime, size) of the XMP
/// sidecar at the moment the catalog last converged with it (PLAN §5
/// incremental rebuild). mtime is whole nanoseconds since the epoch (INTEGER,
/// not REAL: float rounding must never make an unchanged sidecar look
/// changed). NULL = never synced; changed values trigger a re-read on the
/// next read/rebuild (sidecar wins).
const MIGRATION_V4: &str = "
ALTER TABLE files ADD COLUMN sidecar_mtime INTEGER;
ALTER TABLE files ADD COLUMN sidecar_size INTEGER;
CREATE INDEX idx_files_sidecar_sync ON files(sidecar_mtime, sidecar_size);
";

/// TASK-03-003: missing-file records for move/rename re-association
/// (00-010 design §4). When the scanner deletes rows for vanished files it
/// first snapshots them here; a later scan matches new-looking paths against
/// this set by (size, mtime window, lazy hash) and re-creates the file row
/// with the snapshotted user data, so stars/tags/captions/albums follow the
/// file. Rows cascade away with their folder (the folder is really gone).
const MIGRATION_V5: &str = "
CREATE TABLE missing_files (
    id         INTEGER PRIMARY KEY,
    path       TEXT NOT NULL UNIQUE,
    filename   TEXT NOT NULL,
    folder_id  INTEGER NOT NULL,
    size       INTEGER NOT NULL,
    mtime      REAL NOT NULL,
    hash       TEXT,
    media_type TEXT NOT NULL CHECK (media_type IN ('image', 'video')),
    rating     INTEGER,
    caption    TEXT,
    tags       TEXT,
    latitude   REAL,
    longitude  REAL,
    missing_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE missing_file_tags (
    missing_id INTEGER NOT NULL REFERENCES missing_files(id) ON DELETE CASCADE,
    tag        TEXT NOT NULL,
    PRIMARY KEY (missing_id, tag)
);
CREATE INDEX idx_missing_size ON missing_files(size, media_type);
CREATE INDEX idx_missing_hash ON missing_files(hash);
";

/// TASK-03-007: captions become searchable. `files_fts` gains a `caption`
/// column fed from `metadata.caption` (PLAN §5: FTS over filename, folder,
/// caption, …). FTS is derived data, so the table is dropped and rebuilt
/// from the current rows rather than altered. `files` triggers carry the
/// caption along; `metadata` triggers refresh the row when the caption
/// changes (guarded so scan-time EXIF writes don't touch the index).
const MIGRATION_V6: &str = "
DROP TRIGGER files_fts_insert;
DROP TRIGGER files_fts_delete;
DROP TRIGGER files_fts_update;
DROP TABLE files_fts;
CREATE VIRTUAL TABLE files_fts USING fts5(filename, folder, caption);

CREATE TRIGGER files_fts_insert AFTER INSERT ON files BEGIN
    INSERT INTO files_fts(rowid, filename, folder, caption)
    VALUES (NEW.id, NEW.filename, (SELECT path FROM folders WHERE id = NEW.folder_id),
            (SELECT caption FROM metadata WHERE file_id = NEW.id));
END;

CREATE TRIGGER files_fts_delete AFTER DELETE ON files BEGIN
    DELETE FROM files_fts WHERE rowid = OLD.id;
END;

CREATE TRIGGER files_fts_update AFTER UPDATE ON files BEGIN
    DELETE FROM files_fts WHERE rowid = OLD.id;
    INSERT INTO files_fts(rowid, filename, folder, caption)
    VALUES (NEW.id, NEW.filename, (SELECT path FROM folders WHERE id = NEW.folder_id),
            (SELECT caption FROM metadata WHERE file_id = NEW.id));
END;

CREATE TRIGGER files_fts_caption_insert AFTER INSERT ON metadata
WHEN NEW.caption IS NOT NULL BEGIN
    DELETE FROM files_fts WHERE rowid = NEW.file_id;
    INSERT INTO files_fts(rowid, filename, folder, caption)
        SELECT f.id, f.filename, d.path, NEW.caption
        FROM files f JOIN folders d ON d.id = f.folder_id WHERE f.id = NEW.file_id;
END;

CREATE TRIGGER files_fts_caption_update AFTER UPDATE OF caption ON metadata
WHEN OLD.caption IS NOT NEW.caption BEGIN
    DELETE FROM files_fts WHERE rowid = NEW.file_id;
    INSERT INTO files_fts(rowid, filename, folder, caption)
        SELECT f.id, f.filename, d.path, NEW.caption
        FROM files f JOIN folders d ON d.id = f.folder_id WHERE f.id = NEW.file_id;
END;

CREATE TRIGGER files_fts_caption_delete AFTER DELETE ON metadata
WHEN OLD.caption IS NOT NULL BEGIN
    DELETE FROM files_fts WHERE rowid = OLD.file_id;
    INSERT INTO files_fts(rowid, filename, folder, caption)
        SELECT f.id, f.filename, d.path, NULL
        FROM files f JOIN folders d ON d.id = f.folder_id WHERE f.id = OLD.file_id;
END;

INSERT INTO files_fts(rowid, filename, folder, caption)
    SELECT f.id, f.filename, d.path, m.caption
    FROM files f JOIN folders d ON d.id = f.folder_id
    LEFT JOIN metadata m ON m.file_id = f.id;
";

/// TASK-03-008: albums get a stable UUID (the id written to each member's
/// `fb:Albums` bag, 00-010 §2) and a cover pick. The cover is stored as the
/// photo's path, not its row id, so it re-resolves after the catalog is
/// deleted and the files are re-scanned. `albums` / `album_files` are
/// derived cache: the definitions are rebuilt from `albums.json` next to
/// the catalog and the membership from the sidecars.
const MIGRATION_V7: &str = "
ALTER TABLE albums ADD COLUMN uuid TEXT;
ALTER TABLE albums ADD COLUMN cover_path TEXT;
UPDATE albums SET uuid = lower(
    hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)), 2) ||
    '-a' || substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6)))
    WHERE uuid IS NULL;
CREATE UNIQUE INDEX idx_albums_uuid ON albums(uuid);
CREATE INDEX idx_album_files_file ON album_files(file_id);
";

use rusqlite::Connection;

/// TASK-02-003: ffprobe-derived video metadata lives in the same `metadata`
/// row as the EXIF cache (PLAN §2 Video strategy: "extracted at scan time via
/// ffprobe, stored in the catalog `metadata` table"). `video_probe_ok` marks
/// the probe outcome (`NULL` = not probed yet, `1` = ok, `0` = corrupt /
/// failed — a failed probe is logged and marked, never fatal, PLAN §9) and
/// `video_probe_mtime` records the file mtime probed, so a later scan only
/// re-probes videos whose (mtime, size) changed.
const MIGRATION_V3: &str = "
ALTER TABLE metadata ADD COLUMN video_duration REAL;
ALTER TABLE metadata ADD COLUMN video_codec TEXT;
ALTER TABLE metadata ADD COLUMN video_profile TEXT;
ALTER TABLE metadata ADD COLUMN video_rotation INTEGER;
ALTER TABLE metadata ADD COLUMN video_probe_ok INTEGER;
ALTER TABLE metadata ADD COLUMN video_probe_mtime REAL;
CREATE INDEX idx_metadata_video_probe ON metadata(video_probe_ok, video_probe_mtime);
";

/// TASK-03-009 / TASK-03-012: the hidden flag and the catalog trash.
/// `files.hidden` mirrors the photo's `fb:Hidden`; `folders.hidden` mirrors
/// the folder's own `.fotobase` flag and `folders.hidden_eff` is that flag
/// OR any ancestor's (ADR-0007: members inherit), so read paths test one
/// column. All three are rebuildable cache. `trash` holds the soft-deleted
/// organizational actions with their JSON restore payload; `created_at` is
/// unix seconds and drives the undo-window sweep.
const MIGRATION_V8: &str = "
ALTER TABLE files ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
ALTER TABLE folders ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
ALTER TABLE folders ADD COLUMN hidden_eff INTEGER NOT NULL DEFAULT 0;
CREATE TABLE trash (
    id         INTEGER PRIMARY KEY,
    action     TEXT NOT NULL CHECK (action IN ('album_remove', 'hide', 'unlink')),
    path       TEXT NOT NULL,
    name       TEXT NOT NULL,
    detail     TEXT,
    payload    TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_trash_created ON trash(created_at);
";

/// TASK-03-013: tags become searchable. `files_fts` gains a `tags` column
/// (space-joined tag names; the unicode61 tokenizer splits nested
/// `Vacation|Italy` into its segments) next to filename/folder/caption
/// (PLAN §5). FTS is derived data, so the table is dropped and rebuilt from
/// the mirror rows. Every `files` / `metadata.caption` / `file_tags` change
/// refreshes the one FTS row of the affected photo, so the sidecar
/// write-through (and a rebuild from sidecars) keeps search current with no
/// manual step. A trigger firing for a photo whose `files` row is already
/// gone inserts nothing (the SELECT is empty).
const MIGRATION_V9: &str = "
DROP TRIGGER files_fts_insert;
DROP TRIGGER files_fts_delete;
DROP TRIGGER files_fts_update;
DROP TRIGGER files_fts_caption_insert;
DROP TRIGGER files_fts_caption_update;
DROP TRIGGER files_fts_caption_delete;
DROP TABLE files_fts;
CREATE VIRTUAL TABLE files_fts USING fts5(filename, folder, caption, tags);

CREATE TRIGGER files_fts_insert AFTER INSERT ON files BEGIN
    INSERT INTO files_fts(rowid, filename, folder, caption, tags)
    VALUES (NEW.id, NEW.filename, (SELECT path FROM folders WHERE id = NEW.folder_id),
            (SELECT caption FROM metadata WHERE file_id = NEW.id),
            (SELECT group_concat(t.name, ' ') FROM file_tags ft
             JOIN tags t ON t.id = ft.tag_id WHERE ft.file_id = NEW.id));
END;

CREATE TRIGGER files_fts_delete AFTER DELETE ON files BEGIN
    DELETE FROM files_fts WHERE rowid = OLD.id;
END;

CREATE TRIGGER files_fts_update AFTER UPDATE ON files BEGIN
    DELETE FROM files_fts WHERE rowid = OLD.id;
    INSERT INTO files_fts(rowid, filename, folder, caption, tags)
    VALUES (NEW.id, NEW.filename, (SELECT path FROM folders WHERE id = NEW.folder_id),
            (SELECT caption FROM metadata WHERE file_id = NEW.id),
            (SELECT group_concat(t.name, ' ') FROM file_tags ft
             JOIN tags t ON t.id = ft.tag_id WHERE ft.file_id = NEW.id));
END;

CREATE TRIGGER files_fts_caption_insert AFTER INSERT ON metadata
WHEN NEW.caption IS NOT NULL BEGIN
    DELETE FROM files_fts WHERE rowid = NEW.file_id;
    INSERT INTO files_fts(rowid, filename, folder, caption, tags)
        SELECT f.id, f.filename, d.path, NEW.caption,
               (SELECT group_concat(t.name, ' ') FROM file_tags ft
                JOIN tags t ON t.id = ft.tag_id WHERE ft.file_id = f.id)
        FROM files f JOIN folders d ON d.id = f.folder_id WHERE f.id = NEW.file_id;
END;

CREATE TRIGGER files_fts_caption_update AFTER UPDATE OF caption ON metadata
WHEN OLD.caption IS NOT NEW.caption BEGIN
    DELETE FROM files_fts WHERE rowid = NEW.file_id;
    INSERT INTO files_fts(rowid, filename, folder, caption, tags)
        SELECT f.id, f.filename, d.path, NEW.caption,
               (SELECT group_concat(t.name, ' ') FROM file_tags ft
                JOIN tags t ON t.id = ft.tag_id WHERE ft.file_id = f.id)
        FROM files f JOIN folders d ON d.id = f.folder_id WHERE f.id = NEW.file_id;
END;

CREATE TRIGGER files_fts_caption_delete AFTER DELETE ON metadata
WHEN OLD.caption IS NOT NULL BEGIN
    DELETE FROM files_fts WHERE rowid = OLD.file_id;
    INSERT INTO files_fts(rowid, filename, folder, caption, tags)
        SELECT f.id, f.filename, d.path, NULL,
               (SELECT group_concat(t.name, ' ') FROM file_tags ft
                JOIN tags t ON t.id = ft.tag_id WHERE ft.file_id = f.id)
        FROM files f JOIN folders d ON d.id = f.folder_id WHERE f.id = OLD.file_id;
END;

CREATE TRIGGER files_fts_tags_insert AFTER INSERT ON file_tags BEGIN
    DELETE FROM files_fts WHERE rowid = NEW.file_id;
    INSERT INTO files_fts(rowid, filename, folder, caption, tags)
        SELECT f.id, f.filename, d.path, m.caption,
               (SELECT group_concat(t.name, ' ') FROM file_tags ft
                JOIN tags t ON t.id = ft.tag_id WHERE ft.file_id = f.id)
        FROM files f JOIN folders d ON d.id = f.folder_id
        LEFT JOIN metadata m ON m.file_id = f.id WHERE f.id = NEW.file_id;
END;

CREATE TRIGGER files_fts_tags_delete AFTER DELETE ON file_tags BEGIN
    DELETE FROM files_fts WHERE rowid = OLD.file_id;
    INSERT INTO files_fts(rowid, filename, folder, caption, tags)
        SELECT f.id, f.filename, d.path, m.caption,
               (SELECT group_concat(t.name, ' ') FROM file_tags ft
                JOIN tags t ON t.id = ft.tag_id WHERE ft.file_id = f.id)
        FROM files f JOIN folders d ON d.id = f.folder_id
        LEFT JOIN metadata m ON m.file_id = f.id WHERE f.id = OLD.file_id;
END;

INSERT INTO files_fts(rowid, filename, folder, caption, tags)
    SELECT f.id, f.filename, d.path, m.caption,
           (SELECT group_concat(t.name, ' ') FROM file_tags ft
            JOIN tags t ON t.id = ft.tag_id WHERE ft.file_id = f.id)
    FROM files f JOIN folders d ON d.id = f.folder_id
    LEFT JOIN metadata m ON m.file_id = f.id;
";

/// TASK-03-020: artist (`dc:creator`) and copyright (`dc:rights`) mirror
/// the sidecar like the other user-data columns (rebuildable cache). Sidecars
/// synced before the columns existed are marked unsynced so the next rebuild
/// reads the two fields from them.
const MIGRATION_V10: &str = "
ALTER TABLE metadata ADD COLUMN artist TEXT;
ALTER TABLE metadata ADD COLUMN copyright TEXT;
UPDATE files SET sidecar_mtime = NULL, sidecar_size = NULL;
";

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
