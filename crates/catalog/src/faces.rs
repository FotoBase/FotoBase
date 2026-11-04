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

use crate::sync::{sidecar_path, SyncError, SyncResult};
use crate::{Catalog, CatalogError, Result};
use fotobase_metadata::Sidecar;

use rusqlite::OptionalExtension;

/// The face-embedding dimension: the SFace embedder (05-001) emits 128-
/// dimensional vectors, and the sqlite-vec `embeddings` table (TASK-05-008,
/// PLAN §5) is a fixed-width `float[128]` vec0 virtual table. Same number as
/// the `float[128]` column in `migrations::MIGRATION_V18` — keep the two in
/// sync.
pub const EMBEDDING_DIM: usize = 128;

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
    /// The embedding goes into the sqlite-vec virtual table keyed to
    /// `faces.embedding_id` (TASK-05-008); anything but the model's
    /// dimension is refused rather than stored as an unreadable row.
    pub fn record_face(
        &mut self,
        file_id: i64,
        face: &FaceRecord,
        embedding: Option<&[f32]>,
    ) -> Result<i64> {
        if let Some(v) = embedding {
            if v.len() != EMBEDDING_DIM {
                return Err(CatalogError::InvalidQuery(format!(
                    "face embedding must be {EMBEDDING_DIM}-dimensional, got {}",
                    v.len()
                )));
            }
        }
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
                "INSERT INTO embeddings (embedding_id, embedding) VALUES (?1, ?2)",
                rusqlite::params![face_id, bytes],
            )?;
            tx.execute("UPDATE faces SET embedding_id = ?1 WHERE id = ?1", [face_id])?;
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
        let row: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT e.embedding FROM embeddings e \
                 JOIN faces f ON f.embedding_id = e.embedding_id WHERE f.id = ?1",
                [face_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(row.map(|bytes| {
            (
                EMBEDDING_DIM as i64,
                bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect(),
            )
        }))
    }

    /// Delete the face rows (and their sqlite-vec embeddings) of the given
    /// files. vec0 virtual tables do not participate in foreign-key
    /// cascades, so the embedding cleanup is explicit — a deleted photo
    /// must not leave orphan vectors behind. Runs in a SAVEPOINT, so it is
    /// atomic on its own and nests inside the scanner's scan transaction.
    /// Returns how many face rows were removed.
    pub fn delete_faces_for_files(&mut self, file_ids: &[i64]) -> Result<usize> {
        if file_ids.is_empty() {
            return Ok(0);
        }
        let placeholders = "?,".repeat(file_ids.len()).trim_end_matches(',').to_string();
        let sp = self.conn_mut().savepoint()?;
        sp.execute(
            &format!(
                "DELETE FROM embeddings WHERE embedding_id IN \
                 (SELECT id FROM faces WHERE file_id IN ({placeholders}))"
            ),
            rusqlite::params_from_iter(file_ids),
        )?;
        let removed = sp.execute(
            &format!("DELETE FROM faces WHERE file_id IN ({placeholders})"),
            rusqlite::params_from_iter(file_ids),
        )?;
        sp.commit()?;
        Ok(removed)
    }

    /// "Reset faces" for one photo (TASK-05-009, PLAN §2 Picture context
    /// menu): clear every detected/named face of the photo. Sidecar first
    /// (the MWG face regions go, merge-on-write per
    /// docs/sidecar-merge-on-write.md), then the `faces` rows and their
    /// embeddings — the trigger-maintained derivations (person albums, FTS
    /// `people`, the "has faces" filter) follow the row delete. An
    /// unrecognised `Regions` structure is refused: the sidecar and the
    /// catalog stay untouched rather than destroying XMP we do not
    /// understand. Returns how many face rows were removed.
    pub fn reset_faces(&mut self, file_id: i64) -> SyncResult<usize> {
        let path: String = self
            .conn()
            .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))
            .optional()?
            .ok_or(SyncError::UnknownFile(file_id))?;
        let sc_path = sidecar_path(&path);
        // Probe first: the reset must never CREATE a sidecar (cf.
        // clear_recipe), and an unrecognised Regions structure must abort
        // before anything is touched (sidecar-wins rule, §1).
        let mut probe = Sidecar::read(&sc_path)?;
        let regions_removed = match probe.remove_face_regions() {
            None => return Err(SyncError::FaceRegionsUnrecognised),
            Some(n) => n,
        };
        if regions_removed > 0 {
            self.mutate_user_data(file_id, |sc| {
                sc.remove_face_regions();
            })?;
        }
        self.delete_faces_for_files(&[file_id]).map_err(SyncError::from)
    }
}
