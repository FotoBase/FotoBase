//! Live/Motion photo index (TASK-08-017). Detection runs in the scanner;
//! this module stores the result so the grid badge and the viewer never
//! probe a file per view (PLAN §5 derived data — rebuilt by the next scan).

use rusqlite::OptionalExtension;

use crate::{Catalog, Result};

/// What scan-time detection found for one still.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveMedia {
    /// Android Motion Photo: an MP4 embedded in the JPEG at this byte range.
    Motion { offset: u64, len: u64 },
    /// Apple Live Photo: the sibling MOV carrying the same ContentIdentifier.
    Live { video_path: String },
}

/// One image the detection pass may need to look at.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveCandidate {
    pub id: i64,
    pub path: String,
    pub mtime: f64,
    /// mtime at the last detection, `None` when never checked.
    pub checked_mtime: Option<f64>,
    pub has_live: bool,
}

impl LiveCandidate {
    /// Never checked, or the file changed since the last check.
    pub fn needs_check(&self) -> bool {
        self.checked_mtime != Some(self.mtime)
    }
}

impl Catalog {
    /// Record the detection result for one photo (replacing any earlier one)
    /// and the mtime it was checked at. `None` records "checked, not live".
    pub fn set_live_media(
        &mut self,
        file_id: i64,
        checked_mtime: f64,
        live: Option<&LiveMedia>,
    ) -> Result<()> {
        self.conn().execute("DELETE FROM live_media WHERE file_id = ?1", [file_id])?;
        match live {
            Some(LiveMedia::Motion { offset, len }) => {
                self.conn().execute(
                    "INSERT INTO live_media(file_id, kind, clip_offset, clip_len)
                     VALUES (?1, 'motion', ?2, ?3)",
                    rusqlite::params![file_id, *offset as i64, *len as i64],
                )?;
            }
            Some(LiveMedia::Live { video_path }) => {
                self.conn().execute(
                    "INSERT INTO live_media(file_id, kind, video_path) VALUES (?1, 'live', ?2)",
                    rusqlite::params![file_id, video_path],
                )?;
            }
            None => {}
        }
        self.conn().execute(
            "UPDATE files SET live_checked_mtime = ?2 WHERE id = ?1",
            rusqlite::params![file_id, checked_mtime],
        )?;
        Ok(())
    }

    /// The stored detection result for one photo, if it is a Live/Motion photo.
    pub fn live_media(&self, file_id: i64) -> Result<Option<LiveMedia>> {
        let row = self
            .conn()
            .query_row(
                "SELECT kind, video_path, clip_offset, clip_len FROM live_media WHERE file_id = ?1",
                [file_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<i64>>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .optional()?;
        Ok(match row {
            Some((kind, _, Some(offset), Some(len))) if kind == "motion" => {
                Some(LiveMedia::Motion { offset: offset as u64, len: len as u64 })
            }
            Some((kind, Some(video_path), _, _)) if kind == "live" => {
                Some(LiveMedia::Live { video_path })
            }
            _ => None,
        })
    }

    /// Every image under `root` with its detection state. Cheap — catalog
    /// rows only, no file reads; the scanner decides which ones to open.
    pub fn live_candidates_under(&self, root: &str) -> Result<Vec<LiveCandidate>> {
        let sep = std::path::MAIN_SEPARATOR.to_string();
        let mut stmt = self.conn().prepare_cached(
            "SELECT f.id, f.path, f.mtime, f.live_checked_mtime,
                    EXISTS (SELECT 1 FROM live_media lm WHERE lm.file_id = f.id)
             FROM files f JOIN folders d ON d.id = f.folder_id
             WHERE f.media_type = 'image' AND (d.path = ?1 OR d.path LIKE ?1 || ?2 || '%')
             ORDER BY f.id",
        )?;
        let rows = stmt.query_map(rusqlite::params![root, sep], |r| {
            Ok(LiveCandidate {
                id: r.get(0)?,
                path: r.get(1)?,
                mtime: r.get(2)?,
                checked_mtime: r.get(3)?,
                has_live: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// `(path, mtime)` of every video under `root` — the sibling candidates
    /// for Apple Live Photo pairing.
    pub fn video_files_under(&self, root: &str) -> Result<Vec<(String, f64)>> {
        let sep = std::path::MAIN_SEPARATOR.to_string();
        let mut stmt = self.conn().prepare_cached(
            "SELECT f.path, f.mtime FROM files f JOIN folders d ON d.id = f.folder_id
             WHERE f.media_type = 'video' AND (d.path = ?1 OR d.path LIKE ?1 || ?2 || '%')
             ORDER BY f.path",
        )?;
        let rows = stmt.query_map(rusqlite::params![root, sep], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
}
