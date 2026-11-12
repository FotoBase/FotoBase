//! CLIP photo embeddings in the catalog (TASK-07-001, PLAN §5 `embeddings`
//! derived data, §6 P3 Smart search). One 512-d vector per photo in the
//! sqlite-vec `photo_embeddings` table, keyed by file id; the
//! `files.embed_scanned` marker (0 pending / 1 done / 2 failed, the phash
//! sentinel pattern) drives the incremental embedding pass.
//!
//! Embeddings are recomputable derived data: never written to sidecars
//! (PLAN §3 rule 2), recomputed after a catalog rebuild (§9). The vector
//! dimension is pinned to the OpenCLIP ViT-B/32 image projection.

use rusqlite::OptionalExtension;

use crate::{Catalog, CatalogError, Result};

/// The CLIP ViT-B/32 image-projection dimension (`photo_embeddings` is
/// `float[512]`); must match the `fotobase_ai` crate's `clip`
/// module constant.
pub const CLIP_EMBEDDING_DIM: usize = 512;

/// `files.embed_scanned` marker values (the phash sentinel pattern).
const SCANNED_DONE: i64 = 1;
const SCANNED_FAILED: i64 = 2;

impl Catalog {
    /// Unprocessed images, oldest first (a cancelled pass leaves the newest
    /// photos for the next one, like the face pass). Images only — video
    /// frames are out of scope for the embedding pipeline.
    pub fn embed_candidates(&self) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, path FROM files \
             WHERE media_type = 'image' AND embed_scanned = 0 ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.flatten().collect())
    }

    /// Set the processed marker: `true` = done, `false` = failed (parks the
    /// row so one undecodable photo neither blocks the pass forever nor
    /// inflates later counts).
    pub fn mark_embed_scanned(&mut self, file_id: i64, ok: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET embed_scanned = ?1 WHERE id = ?2",
            rusqlite::params![if ok { SCANNED_DONE } else { SCANNED_FAILED }, file_id],
        )?;
        Ok(())
    }

    /// Clear the processed marker so a later pass retries a file (e.g.
    /// after a decoder fix). Explicit — nothing retries automatically.
    pub fn reset_embed_scan(&self, file_id: i64) -> Result<()> {
        self.conn.execute("UPDATE files SET embed_scanned = 0 WHERE id = ?1", [file_id])?;
        Ok(())
    }

    /// Store one photo's embedding in the sqlite-vec table. Anything but
    /// the model's dimension is refused rather than stored as an
    /// unreadable row.
    pub fn store_photo_embedding(&mut self, file_id: i64, embedding: &[f32]) -> Result<()> {
        if embedding.len() != CLIP_EMBEDDING_DIM {
            return Err(CatalogError::InvalidQuery(format!(
                "CLIP embedding must be {CLIP_EMBEDDING_DIM}-dimensional, got {}",
                embedding.len()
            )));
        }
        let bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
        // vec0 virtual tables do not support upsert semantics: replace is
        // an explicit delete + insert in one transaction.
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM photo_embeddings WHERE file_id = ?1", [file_id])?;
        tx.execute(
            "INSERT INTO photo_embeddings (file_id, embedding) VALUES (?1, ?2)",
            rusqlite::params![file_id, bytes],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The stored embedding of one photo, or `None`.
    pub fn photo_embedding(&self, file_id: i64) -> Result<Option<Vec<f32>>> {
        let bytes: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT embedding FROM photo_embeddings WHERE file_id = ?1",
                [file_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(bytes.map(|b| b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()))
    }

    /// The ANN query path (TASK-07-002, PLAN §4 Vector search): the k
    /// nearest stored photo embeddings to `query`, ordered by ascending
    /// cosine distance. `query` must be the L2-normalized CLIP projection
    /// (image embedding or text encoding — same 512-d space).
    pub fn search_photo_embeddings(&self, query: &[f32], k: usize) -> Result<Vec<(i64, f32)>> {
        if query.len() != CLIP_EMBEDDING_DIM {
            return Err(CatalogError::InvalidQuery(format!(
                "CLIP query must be {CLIP_EMBEDDING_DIM}-dimensional, got {}",
                query.len()
            )));
        }
        let bytes: Vec<u8> = query.iter().flat_map(|f| f.to_le_bytes()).collect();
        let mut stmt = self.conn.prepare_cached(
            "SELECT file_id, distance FROM photo_embeddings \
             WHERE embedding MATCH ?1 AND k = ?2 ORDER BY distance",
        )?;
        let rows = stmt.query_map(rusqlite::params![bytes, k as i64], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, f32>(1)?))
        })?;
        Ok(rows.flatten().collect())
    }

    /// Clear all photo embeddings and reset every `embed_scanned` marker to
    /// pending: a full catalog rebuild re-derives the embeddings on the next
    /// pass (TASK-07-002, PLAN §9 — derived data is recomputed, §3 rule 2 —
    /// never migrated or written to sidecars).
    pub fn reset_all_photo_embeddings(&mut self) -> Result<()> {
        self.conn.execute("DELETE FROM photo_embeddings", [])?;
        self.conn.execute("UPDATE files SET embed_scanned = 0 WHERE embed_scanned != 0", [])?;
        Ok(())
    }

    /// Drop the photo embeddings of the given files (vec0 virtual tables do
    /// not participate in FK cascades — the explicit-cleanup pattern from
    /// TASK-05-008).
    pub fn delete_photo_embeddings(&mut self, file_ids: &[i64]) -> Result<usize> {
        if file_ids.is_empty() {
            return Ok(0);
        }
        let placeholders = "?,".repeat(file_ids.len()).trim_end_matches(',').to_string();
        let removed = self.conn.execute(
            &format!("DELETE FROM photo_embeddings WHERE file_id IN ({placeholders})"),
            rusqlite::params_from_iter(file_ids),
        )?;
        Ok(removed)
    }
}
