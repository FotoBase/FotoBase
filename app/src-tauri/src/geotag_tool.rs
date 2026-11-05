//! Geotag tool app-side runner (TASK-06-006). Thin wrapper around the
//! catalog crate's batch: the job-queue job opens its own catalog
//! connection (the UI's connection is not `Send`) and streams counters on
//! the `geotag-progress` event — no image bytes over IPC (PLAN §3 rule 3).

use std::path::Path;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{Catalog, GeotagTarget, GeotagToolReport};

/// Tauri event the tool reports progress on.
pub const GEOTAG_EVENT: &str = "geotag-progress";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct GeotagProgress {
    pub done: usize,
    pub total: usize,
}

/// Run one geotag pass against the catalog at `catalog_path`, on the job
/// queue's blocking thread. Cancellation is a plain flag the caller bridges
/// from the queue's cancel token; stopping happens between photos.
pub fn run(
    catalog_path: &Path,
    target: &GeotagTarget,
    latitude: f64,
    longitude: f64,
    cancel: &AtomicBool,
    emit: &dyn Fn(GeotagProgress),
) -> Result<GeotagToolReport, String> {
    let mut cat = Catalog::open(catalog_path).map_err(|e| format!("open catalog: {e}"))?;
    cat.geotag_tool_run(target, latitude, longitude, cancel, &|done, total| {
        emit(GeotagProgress { done, total })
    })
    .map_err(|e| e.to_string())
}

/// Undo the latest geotag batch, same connection rules as `run`.
pub fn undo(
    catalog_path: &Path,
    _cancel: &AtomicBool,
    emit: &dyn Fn(GeotagProgress),
) -> Result<usize, String> {
    let mut cat = Catalog::open(catalog_path).map_err(|e| format!("open catalog: {e}"))?;
    cat.undo_geotag(&|done, total| emit(GeotagProgress { done, total })).map_err(|e| e.to_string())
}
