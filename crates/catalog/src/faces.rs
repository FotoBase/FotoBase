//! Face-detection catalog side (TASK-05-002 / TASK-05-010, PLAN §6 P3
//! People, §5 Authority). Everything here is derived data: the catalog is a
//! rebuildable cache and faces/embeddings are never written to sidecars
//! (PLAN §3 rule 2) — the MWG region write itself is TASK-05-008's job and
//! follows `docs/sidecar-merge-on-write.md` there.
//!
//! The per-folder toggle lives in `folders.face_detect`: the Folder Manager
//! (TASK-05-010) flips it, and the face-job scheduler (TASK-05-002) reads it
//! before scheduling — a disabled folder schedules nothing and its existing
//! faces are left alone.

use crate::{Catalog, Result};

use rusqlite::OptionalExtension;

/// One detected face, in the source image's pixel coordinates, ready to
/// persist. `landmarks` are the five detector points (right eye, left eye,
/// nose tip, right/left mouth corner) the embedder aligns with — stored so a
/// later re-embed or the MWG write needs no re-detection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceRecord {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub landmarks: [f32; 10],
    pub confidence: f32,
}

/// One stored face row, as the UI / MWG writer consumes it. `landmarks` is
/// `None` when the row predates landmark storage.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FaceRow {
    pub id: i64,
    pub file_id: i64,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub landmarks: Option<Vec<f32>>,
    pub confidence: f32,
}

/// `files.face_scanned` marker values.
const SCANNED_DONE: i64 = 1;
const SCANNED_FAILED: i64 = 2;

fn faces_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FaceRow> {
    let landmarks: Option<Vec<u8>> = row.get(6)?;
    Ok(FaceRow {
        id: row.get(0)?,
        file_id: row.get(1)?,
        x: row.get(2)?,
        y: row.get(3)?,
        width: row.get(4)?,
        height: row.get(5)?,
        landmarks: landmarks
            .map(|bytes| bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()),
        confidence: row.get(7)?,
    })
}

impl Catalog {
    /// Persist the per-folder face-detection toggle (TASK-05-010). Returns
    /// `false` when no folder row has that path — the caller decides whether
    /// that is an error (the Folder Manager treats it as a stale dialog).
    pub fn set_folder_face_detection(&self, path: &str, enabled: bool) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE folders SET face_detect = ?2 WHERE path = ?1",
            rusqlite::params![path, i64::from(enabled)],
        )?;
        Ok(changed > 0)
    }

    /// The folder's toggle state; `None` when the folder is unknown.
    pub fn folder_face_detection(&self, path: &str) -> Result<Option<bool>> {
        let v: Option<i64> = self
            .conn
            .query_row("SELECT face_detect FROM folders WHERE path = ?1", [path], |r| r.get(0))
            .optional()?;
        Ok(v.map(|v| v != 0))
    }

    /// Ids of the folders with face detection enabled — the scheduler's
    /// source of truth (TASK-05-002: "per-folder toggle read from Folder
    /// Manager settings before scheduling").
    pub fn face_enabled_folder_ids(&self) -> Result<Vec<i64>> {
        let mut stmt =
            self.conn.prepare_cached("SELECT id FROM folders WHERE face_detect = 1 ORDER BY id")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.flatten().collect())
    }

    /// Unprocessed images inside the given folders, oldest first (a
    /// cancelled pass leaves the newest photos for the next one, like the
    /// phash pass). Images only — video frames are out of scope for the
    /// face pipeline.
    pub fn face_candidates(&self, folder_ids: &[i64]) -> Result<Vec<(i64, String)>> {
        if folder_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = "?,".repeat(folder_ids.len()).trim_end_matches(',').to_string();
        let sql = format!(
            "SELECT id, path FROM files \
             WHERE folder_id IN ({placeholders}) AND media_type = 'image' \
             AND face_scanned = 0 ORDER BY id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let params = rusqlite::params_from_iter(folder_ids);
        let rows = stmt.query_map(params, |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.flatten().collect())
    }

    /// Store one detected face (and its embedding, when the embedder ran).
    /// Face row and embedding land in one transaction: a face without its
    /// embedding where one was expected would corrupt clustering later.
    pub fn record_face(
        &mut self,
        file_id: i64,
        face: &FaceRecord,
        embedding: Option<&[f32]>,
    ) -> Result<i64> {
        let tx = self.conn_mut().transaction()?;
        let lm: Vec<u8> = face.landmarks.iter().flat_map(|f| f.to_le_bytes()).collect();
        tx.execute(
            "INSERT INTO faces (file_id, x, y, width, height, landmarks, confidence) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                file_id,
                face.x,
                face.y,
                face.width,
                face.height,
                lm,
                face.confidence
            ],
        )?;
        let face_id = tx.last_insert_rowid();
        if let Some(v) = embedding {
            let bytes: Vec<u8> = v.iter().flat_map(|f| f.to_le_bytes()).collect();
            tx.execute(
                "INSERT INTO embeddings (face_id, dim, vector) VALUES (?1, ?2, ?3)",
                rusqlite::params![face_id, v.len() as i64, bytes],
            )?;
        }
        tx.commit()?;
        Ok(face_id)
    }

    /// Mark a file's detection pass finished (`ok`) or failed. A "done" file
    /// with zero faces stays done — re-detecting every empty photo on every
    /// pass would make the pipeline quadratic in passes.
    pub fn mark_face_scanned(&self, file_id: i64, ok: bool) -> Result<()> {
        let marker = if ok { SCANNED_DONE } else { SCANNED_FAILED };
        self.conn.execute(
            "UPDATE files SET face_scanned = ?1 WHERE id = ?2",
            rusqlite::params![marker, file_id],
        )?;
        Ok(())
    }

    /// Clear the processed marker so a later pass retries a file (e.g. after
    /// a decoder fix). Explicit — nothing retries automatically.
    pub fn reset_face_scan(&self, file_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET face_scanned = 0 WHERE id = ?1",
            rusqlite::params![file_id],
        )?;
        Ok(())
    }

    /// All stored faces of one file, row-id ordered (stable read order).
    pub fn faces_for_file(&self, file_id: i64) -> Result<Vec<FaceRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, file_id, x, y, width, height, landmarks, confidence \
             FROM faces WHERE file_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([file_id], faces_from_row)?;
        Ok(rows.flatten().collect())
    }

    /// The stored embedding of one face: `(dim, vector)` or `None`.
    pub fn face_embedding(&self, face_id: i64) -> Result<Option<(i64, Vec<f32>)>> {
        let row: Option<(i64, Vec<u8>)> = self
            .conn
            .query_row("SELECT dim, vector FROM embeddings WHERE face_id = ?1", [face_id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        Ok(row.map(|(dim, bytes)| {
            (dim, bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
        }))
    }
}
