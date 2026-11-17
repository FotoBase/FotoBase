//! The sharpness/quality scoring pass as a job-queue job (TASK-07-006, PLAN
//! §6 P3 AI culling, §3 rule 4, §5 derived data).
//!
//! A full-library pass walks every cataloged image, computes the cheap
//! Laplacian-variance + exposure score over the decoded preview and stores
//! it in the catalog's `photo_quality` table. `files.quality_scanned` marks
//! 0 pending / 1 done / 2 failed (the phash sentinel pattern), so a pass is
//! incremental and a cancelled pass leaves valid state. Scores are derived
//! data in the catalog (PLAN §5) — never written to sidecars, recomputed
//! after a rebuild. No model, no download: the scorer is pure math on the
//! decoded preview.
//!
//! Battery: the queue this runs on shares the app's [`fotobase_jobs::
//! BatteryHook`] — on battery the dequeue pauses (§10). Background priority
//! only: visible thumbnails are never starved.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use fotobase_catalog::Catalog;
use serde::Serialize;

/// Tauri event the scoring pass reports progress on (§3: plain counters,
/// no image bytes).
pub const CULLING_EVENT: &str = "culling-progress";

/// One full pass over the unprocessed images.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct QualityReport {
    /// Photos scored on this pass.
    pub scored: usize,
    /// Photos that could not be decoded or scored (marked failed; a later
    /// explicit reset retries them).
    pub failed: usize,
}

/// The scoring backend. Pure-math production engine below; a fake stands in
/// for tests, and a learned quality model can implement the same interface
/// later (TASK-07-006 technical spec).
pub trait QualityEngine: Send {
    fn score(&mut self, rgb: &[u8], width: u32, height: u32) -> Result<f64, String>;
}

/// Production engine: Laplacian-variance sharpness + exposure heuristics on
/// the decoded preview's gray conversion (fotobase-ai, no model needed).
pub struct PureQualityEngine;

impl QualityEngine for PureQualityEngine {
    fn score(&mut self, rgb: &[u8], width: u32, height: u32) -> Result<f64, String> {
        let gray = fotobase_ai::gray_from_rgb(rgb, width as usize, height as usize);
        Ok(fotobase_ai::quality_score(&gray, width as usize, height as usize).total as f64)
    }
}

/// One full-library scoring pass. Cooperative cancellation: a cancelled pass
/// leaves valid state — unprocessed rows stay pending, a later pass picks
/// them up.
pub fn run_quality_pass(
    cat: &mut Catalog,
    engine: &mut dyn QualityEngine,
    cancel: &AtomicBool,
) -> Result<QualityReport, String> {
    let mut report = QualityReport::default();
    for (file_id, path) in cat.quality_candidates().map_err(|e| e.to_string())? {
        if cancel.load(Ordering::Relaxed) {
            return Ok(report);
        }
        match fotobase_render::decode_rgb(std::path::Path::new(&path)) {
            Ok((rgb, w, h)) => match engine.score(&rgb, w, h) {
                Ok(score) => {
                    cat.store_quality_score(file_id, score).map_err(|e| e.to_string())?;
                    cat.mark_quality_scanned(file_id, true).map_err(|e| e.to_string())?;
                    report.scored += 1;
                }
                Err(_) => {
                    cat.mark_quality_scanned(file_id, false).map_err(|e| e.to_string())?;
                    report.failed += 1;
                }
            },
            Err(_) => {
                // Park the row (sentinel) so one undecodable photo neither
                // blocks the pass forever nor inflates later counts.
                cat.mark_quality_scanned(file_id, false).map_err(|e| e.to_string())?;
                report.failed += 1;
            }
        }
    }
    Ok(report)
}

/// Monotonic sequence for scoring-pass job keys (one full pass at a time
/// coalesces; a re-trigger while a pass runs attaches to it).
static PASS_SEQ: AtomicU64 = AtomicU64::new(0);

/// Job key for the next full-library pass.
pub fn job_key() -> String {
    format!("quality-pass-{}", PASS_SEQ.fetch_add(1, Ordering::Relaxed))
}
