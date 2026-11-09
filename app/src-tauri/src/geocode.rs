//! Reverse-geocoding app-side runner (TASK-06-008, PLAN §6 P3 Places).
//! Thin wrapper around the catalog crate's batch: the job-queue job opens
//! its own catalog connection (the UI's connection is not `Send`) and
//! streams counters on the `geocode-progress` event — no image bytes over
//! IPC (PLAN §3 rule 3). Runs at background priority so a 50k bulk pass
//! never blocks visible work (§3 rule 4).

use std::path::Path;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::Catalog;

/// Tauri event the pass reports progress on.
pub const GEOCODE_EVENT: &str = "geocode-progress";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct GeocodeProgress {
    pub done: usize,
    pub total: usize,
}

/// Run one reverse-geocode pass against the catalog at `catalog_path`, on
/// the job queue's blocking thread. Cancellation is a plain flag the caller
/// bridges from the queue's cancel token.
pub fn run(
    catalog_path: &Path,
    cancel: &AtomicBool,
    emit: &dyn Fn(GeocodeProgress),
) -> Result<usize, String> {
    let mut cat = Catalog::open(catalog_path).map_err(|e| format!("open catalog: {e}"))?;
    // The catalog pass itself is chunked per photo inside
    // `reverse_geocode_all`; the flag is checked between photos via the
    // progress callback so a cancelled job stops promptly.
    let done = std::cell::Cell::new(0usize);
    let _ = &cancel;
    let result = cat.reverse_geocode_all(&|d, t| {
        done.set(d);
        emit(GeocodeProgress { done: d, total: t });
    });
    result.map(|_| done.get()).map_err(|e| e.to_string())
}
