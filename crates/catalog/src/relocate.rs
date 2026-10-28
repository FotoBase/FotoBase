//! Catalog side of batch rename/move (TASK-03-011, 00-010 §4.5): read the
//! rows a batch operates on and re-point one row at its new path. The file
//! operations themselves live in the scanner crate.

use rusqlite::OptionalExtension;

use crate::{Catalog, Result};

/// What a batch needs to know about one photo.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchFile {
    pub id: i64,
    pub path: String,
    /// ISO (or EXIF-style) capture stamp, if known.
    pub taken_at: Option<String>,
    pub mtime: f64,
}

impl Catalog {
    /// The rows for `ids`, in the order given (the batch counter follows the
    /// user's selection order). Unknown ids are skipped.
    pub fn batch_files(&self, ids: &[i64]) -> Result<Vec<BatchFile>> {
        let mut stmt = self
            .conn()
            .prepare_cached("SELECT id, path, taken_at, mtime FROM files WHERE id = ?1")?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let row = stmt
                .query_row([id], |r| {
                    Ok(BatchFile {
                        id: r.get(0)?,
                        path: r.get(1)?,
                        taken_at: r.get(2)?,
                        mtime: r.get(3)?,
                    })
                })
                .optional()?;
            out.extend(row);
        }
        Ok(out)
    }

    /// Re-point a row at its new location after the file was moved or
    /// renamed. The row keeps its id, so ratings, tags, captions and album
    /// membership stay attached; folder, filename (and through the triggers
    /// the FTS entry), size and mtime follow the file so the next scan
    /// matches it by (mtime, size) instead of seeing a new photo.
    ///
    /// A scan racing the batch may already have re-associated the moved file
    /// under a fresh row (03-003): then the row at `new_path` is accepted as
    /// the result and nothing is changed.
    pub fn relocate_file(
        &mut self,
        file_id: i64,
        new_path: &str,
        new_dir: &str,
        new_size: i64,
        new_mtime: f64,
    ) -> Result<()> {
        let filename = std::path::Path::new(new_path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO folders(path) VALUES (?1) ON CONFLICT(path) DO NOTHING",
            [new_dir],
        )?;
        let folder_id: i64 =
            tx.query_row("SELECT id FROM folders WHERE path = ?1", [new_dir], |r| r.get(0))?;
        let updated = tx.execute(
            "UPDATE files SET folder_id = ?1, path = ?2, filename = ?3, size = ?4, mtime = ?5
             WHERE id = ?6",
            rusqlite::params![folder_id, new_path, filename, new_size, new_mtime, file_id],
        )?;
        if updated == 0 {
            let reassociated: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM files WHERE path = ?1)",
                [new_path],
                |r| r.get(0),
            )?;
            if !reassociated {
                return Err(rusqlite::Error::QueryReturnedNoRows.into());
            }
        }
        tx.commit()?;
        Ok(())
    }
}
