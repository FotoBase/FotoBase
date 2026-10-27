//! Missing-file records for move/rename re-association (TASK-03-003, 00-010
//! design §4). When a catalogued file vanishes the scanner snapshots its row
//! here before deleting it; a later scan matches new-looking paths against
//! this set by (size, mtime window) then lazily by content hash, and
//! re-creates the file row with the snapshotted user data — so stars, tags,
//! captions and albums follow the file instead of being lost or duplicated.

use rusqlite::OptionalExtension;

use crate::{Catalog, Result};

/// One vanished file, as recorded at the moment it disappeared.
#[derive(Debug, Clone, PartialEq)]
pub struct MissingFile {
    pub id: i64,
    pub path: String,
    pub size: i64,
    pub mtime: f64,
    pub hash: Option<String>,
    pub media_type: String,
    pub rating: Option<i64>,
    pub caption: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

const MISSING_COLS: &str =
    "id, path, size, mtime, hash, media_type, rating, caption, latitude, longitude";

impl Catalog {
    /// Snapshot the given file rows (about to be deleted by the scanner)
    /// into `missing_files`, including their user data (rating, caption,
    /// tags, GPS). Idempotent per path: a path already recorded is refreshed
    /// to the latest known state. Returns how many rows were snapshotted.
    pub fn snapshot_missing(&mut self, paths: &[String]) -> Result<usize> {
        let mut done = 0;
        for path in paths {
            done += self.conn().execute(
                "INSERT OR REPLACE INTO missing_files
                    (path, filename, folder_id, size, mtime, hash, media_type,
                     rating, caption, latitude, longitude)
                 SELECT f.path, f.filename, f.folder_id, f.size, f.mtime, f.hash, f.media_type,
                        m.rating, m.caption, m.latitude, m.longitude
                 FROM files f LEFT JOIN metadata m ON m.file_id = f.id
                 WHERE f.path = ?1",
                [path],
            )?;
            let missing_id: i64 = self.conn().query_row(
                "SELECT id FROM missing_files WHERE path = ?1",
                [path],
                |r| r.get(0),
            )?;
            self.conn().execute(
                "INSERT OR IGNORE INTO missing_file_tags (missing_id, tag)
                 SELECT ?1, t.name FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
                 WHERE ft.file_id = (SELECT id FROM files WHERE path = ?2)",
                rusqlite::params![missing_id, path],
            )?;
        }
        Ok(done)
    }

    /// Missing records of one (size, media_type) class — the cheap first
    /// probe tier. Path-ordered for deterministic matching.
    pub fn missing_with_size(&self, size: i64, media_type: &str) -> Result<Vec<MissingFile>> {
        let mut stmt = self.conn().prepare_cached(&format!(
            "SELECT {MISSING_COLS} FROM missing_files
             WHERE size = ?1 AND media_type = ?2 ORDER BY path"
        ))?;
        let rows = stmt.query_map(rusqlite::params![size, media_type], map_missing)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// All missing records matching a content hash — the authoritative tier.
    pub fn missing_with_hash(&self, hash: &str, media_type: &str) -> Result<Vec<MissingFile>> {
        let mut stmt = self.conn().prepare_cached(&format!(
            "SELECT {MISSING_COLS} FROM missing_files
             WHERE hash = ?1 AND media_type = ?2 ORDER BY path"
        ))?;
        let rows = stmt.query_map(rusqlite::params![hash, media_type], map_missing)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// A new-looking path that is actually the old file back at its original
    /// location (user moved it back): forget the record.
    pub fn drop_missing(&mut self, path: &str) -> Result<()> {
        self.conn().execute("DELETE FROM missing_files WHERE path = ?1", [path])?;
        Ok(())
    }

    /// Re-create the file row from a missing record at its new location,
    /// restoring the snapshotted user data. The catalogue id is new (the old
    /// row was deleted); identity comes from the content, not the rowid.
    /// The original filename is restored so the sidecar name stays consistent.
    pub fn reassociate_file(
        &mut self,
        missing_id: i64,
        new_path: &str,
        new_folder: &str,
        new_size: i64,
        new_mtime: f64,
    ) -> Result<Option<i64>> {
        let rec = self
            .conn()
            .query_row(
                &format!("SELECT {MISSING_COLS} FROM missing_files WHERE id = ?1"),
                [missing_id],
                map_missing,
            )
            .optional()?;
        let Some(rec) = rec else { return Ok(None) };

        let tags: Vec<String> = {
            let mut stmt = self
                .conn()
                .prepare("SELECT tag FROM missing_file_tags WHERE missing_id = ?1 ORDER BY tag")?;
            let rows = stmt.query_map([missing_id], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        self.conn().execute("DELETE FROM missing_files WHERE id = ?1", [missing_id])?;
        let folder_id: i64 =
            self.conn()
                .query_row("SELECT id FROM folders WHERE path = ?1", [new_folder], |r| r.get(0))?;
        let original_name = std::path::Path::new(&rec.path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| {
                std::path::Path::new(new_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_string()
            });

        self.conn().execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, hash, media_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                folder_id,
                new_path,
                original_name,
                new_size,
                new_mtime,
                rec.hash,
                rec.media_type
            ],
        )?;
        let file_id = self.conn().last_insert_rowid();

        // Restore the snapshotted user data (the sidecar is authoritative and
        // follows the original; this row keeps the grid correct even before
        // the next sidecar read).
        self.conn().execute(
            "INSERT INTO metadata (file_id, rating, caption, latitude, longitude)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![file_id, rec.rating, rec.caption, rec.latitude, rec.longitude],
        )?;
        for name in tags {
            self.conn().execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [&name])?;
            self.conn().execute(
                "INSERT OR IGNORE INTO file_tags (file_id, tag_id)
                 VALUES (?1, (SELECT id FROM tags WHERE name = ?2))",
                rusqlite::params![file_id, name],
            )?;
        }
        Ok(Some(file_id))
    }
}

fn map_missing(r: &rusqlite::Row<'_>) -> rusqlite::Result<MissingFile> {
    Ok(MissingFile {
        id: r.get(0)?,
        path: r.get(1)?,
        size: r.get(2)?,
        mtime: r.get(3)?,
        hash: r.get(4)?,
        media_type: r.get(5)?,
        rating: r.get(6)?,
        caption: r.get(7)?,
        latitude: r.get(8)?,
        longitude: r.get(9)?,
    })
}
