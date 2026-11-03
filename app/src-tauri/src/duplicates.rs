//! Near-duplicate finder, app half (TASK-04-019, PLAN §2 2026 additions,
//! §6 P2b).
//!
//! Hashing is heavy work and runs in the bounded job queue at background
//! priority (PLAN §3 rule 4) — the UI never blocks on a 50k catalog. The
//! perceptual hashes live in `files.phash` (derived data, recomputable —
//! never written to sidecars). Grouping and review are cheap catalog
//! queries; nothing here deletes anything: the delete command routes each
//! user-confirmed id through the file-menu "Delete from disk" path (OS
//! trash, catalog row dropped after the file is safely trashed).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use fotobase_catalog::{Catalog, DuplicateGroup as CatalogGroup};
use serde::Serialize;

/// Tauri event the hashing job reports progress on (§3: plain counters, no
/// image bytes).
pub const DUPES_EVENT: &str = "dupes-progress";

/// Background hashing outcome, one pass over the unhashed images.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HashReport {
    /// Files hashed on this pass.
    pub hashed: usize,
    /// Files that could not be decoded (left unhashed; a later pass or a
    /// re-scan retries them).
    pub failed: usize,
}

/// Hash every image still missing a perceptual hash. Cooperative
/// cancellation: a cancelled pass leaves valid state — unhashed rows stay
/// unhashed and a later pass picks them up.
pub fn hash_missing(cat: &mut Catalog, cancel: &AtomicBool) -> Result<HashReport, String> {
    let mut report = HashReport::default();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(report);
        }
        let batch = cat.phash_candidates().map_err(|e| e.to_string())?;
        if batch.is_empty() {
            return Ok(report);
        }
        for (id, path) in batch {
            if cancel.load(Ordering::Relaxed) {
                return Ok(report);
            }
            match fotobase_render::perceptual_hash(std::path::Path::new(&path)) {
                Ok(hash) => {
                    cat.set_phash(id, &hash).map_err(|e| e.to_string())?;
                    report.hashed += 1;
                }
                Err(_) => {
                    // Mark the row so one undecodable photo neither blocks
                    // the pass forever (it is re-selected every pass) nor
                    // inflates the count: store a sentinel 8-byte hash of
                    // all-ones, far from any real photo's hash.
                    cat.set_phash(id, &[0xFF; 8]).map_err(|e| e.to_string())?;
                    report.failed += 1;
                }
            }
        }
    }
}

/// The ids a confirmed delete acts on, resolved to paths — the file-menu
/// trash move needs the path, the catalog row drops by id. Unknown ids
/// (already deleted by a concurrent action) are skipped.
pub fn resolve_for_deletion(cat: &Catalog, ids: &[i64]) -> Result<Vec<(i64, String)>, String> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(path) = cat.file_path(*id).map_err(|e| e.to_string())? {
            out.push((*id, path));
        }
    }
    Ok(out)
}

/// IPC payload: one reviewable group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DuplicateGroupInfo {
    pub file_ids: Vec<i64>,
    pub paths: Vec<String>,
    pub filenames: Vec<String>,
    pub hashes: Vec<Option<String>>,
}

impl From<CatalogGroup> for DuplicateGroupInfo {
    fn from(g: CatalogGroup) -> Self {
        Self { file_ids: g.file_ids, paths: g.paths, filenames: g.filenames, hashes: g.hashes }
    }
}

/// Monotonic sequence for hashing job keys (one find run at a time).
static DUPES_SEQ: AtomicU64 = AtomicU64::new(0);

/// Build a unique job key; concurrent "find duplicates" requests get
/// separate runs (results converge anyway — hashing is idempotent).
pub fn job_key() -> String {
    format!("dupes-hash-{}", DUPES_SEQ.fetch_add(1, Ordering::Relaxed))
}
