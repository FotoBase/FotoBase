//! Rebuild catalog (TASK-01-023, PLAN §2L "Rebuild cache/database", §5).
//!
//! The catalog is a rebuildable cache; user data lives in XMP sidecars.
//! Rebuild re-walks every watched folder in full mode — the (mtime, size)
//! skip of the incremental default is disabled, so derived rows are
//! repopulated from what is on disk — and reports progress so the UI can
//! show folders/files processed while staying responsive.
//!
//! Sidecar-backed user data in the catalog survives by construction: the
//! scanner upserts `files` rows by path (stable file ids), so
//! `metadata` / `file_tags` rows keyed by `file_id` are never dropped. The
//! scanner also converges each re-walked file's sidecar into the catalog
//! (03-005), so user data lost with a deleted catalog is restored from XMP.
//!
//! Originals and sidecars are only ever read here, never written.

use std::path::PathBuf;

use fotobase_catalog::{Catalog, CatalogError};
use fotobase_scanner::{scan_folder_with, ScanOptions};
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use tauri::{Emitter, Manager};

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-rebuild";

/// Tauri event the rebuild reports progress on (§3: progress events, no
/// image bytes — this payload is plain counters).
pub const REBUILD_EVENT: &str = "rebuild-progress";

/// Incremental progress for the UI.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RebuildProgress {
    /// Watched folders already re-scanned.
    pub folders_done: usize,
    /// Watched folders total.
    pub folders_total: usize,
    /// Media files walked so far, across all folders.
    pub files_done: usize,
    /// Folder currently being walked.
    pub current_folder: String,
}

/// Aggregated outcome of one rebuild run.
#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct RebuildReport {
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    pub removed: usize,
    pub cancelled: bool,
}

/// Watched folders the rebuild walks: every top-level folder in the catalog.
/// The `folders` table holds a row per directory ever seen, so nested rows
/// are filtered out — scanning a parent already covers its children, and a
/// full rebuild must not walk (or re-count) the same tree three times.
pub fn watched_folders(cat: &Catalog) -> Vec<String> {
    let mut stmt =
        cat.conn().prepare("SELECT path FROM folders ORDER BY path").expect("folders query");
    let all: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .expect("folders query map")
        .flatten()
        .collect();
    let mut roots: Vec<String> = Vec::new();
    for path in all {
        // Sorted order guarantees a parent is seen before its children.
        if !roots.iter().any(|r| is_within(&path, r)) {
            roots.push(path);
        }
    }
    roots
}

/// True when `path` is `root` itself or inside it. Separator-agnostic
/// (both `/` and `\` count as the boundary — catalog rows store paths as
/// the caller spelled them; see [`fotobase_catalog::is_within`]).
fn is_within(path: &str, root: &str) -> bool {
    path == root || fotobase_catalog::is_within(path, root)
}

/// Full rebuild: re-scan each watched folder with the skip disabled.
/// `emit` receives progress per directory plus one final event per folder;
/// `files_done` accumulates across folders.
pub fn rebuild(
    cat: &mut Catalog,
    roots: &[String],
    emit: &dyn Fn(RebuildProgress),
) -> Result<RebuildReport, CatalogError> {
    let total = roots.len();
    // `scan_folder_with` only takes an `Fn`, so per-run counters live behind
    // interior mutability (single-threaded here; the caller decides on
    // threading, not this function).
    let files_done = std::cell::Cell::new(0usize);
    let folder_files = std::cell::Cell::new(0usize);

    let mut totals = RebuildReport::default();
    for (done, root) in roots.iter().enumerate() {
        let root_path = PathBuf::from(root);
        let current = root.clone();
        folder_files.set(0);
        let report = scan_folder_with(
            cat,
            &root_path,
            &AtomicBool::new(false),
            &ScanOptions { full: true },
            &|p| {
                // The callback's counts are cumulative within this folder;
                // convert the delta and accumulate across folders.
                let delta = p.files_seen - folder_files.get();
                folder_files.set(p.files_seen);
                files_done.set(files_done.get() + delta);
                emit(RebuildProgress {
                    folders_done: done,
                    folders_total: total,
                    files_done: files_done.get(),
                    current_folder: current.clone(),
                });
            },
        )?;

        totals.added += report.added;
        totals.updated += report.updated;
        totals.skipped += report.skipped;
        totals.removed += report.removed;
        totals.cancelled |= report.cancelled;

        // Folder completed: the UI's indicator advances even when the walk
        // found nothing to report.
        emit(RebuildProgress {
            folders_done: done + 1,
            folders_total: total,
            files_done: files_done.get(),
            current_folder: current,
        });
    }
    Ok(totals)
}

/// One rebuild run, driven by the job queue: open the shared catalog,
/// re-scan every watched folder, and emit [`REBUILD_EVENT`] as it goes.
/// Runs on a blocking thread; the UI stays responsive and renders progress
/// from the events.
pub fn run(app: &tauri::AppHandle) -> Result<RebuildReport, String> {
    let state = app.state::<crate::AppState>();
    let mut guard = state.catalog.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))?;
    if guard.is_none() {
        *guard =
            Some(Catalog::open(&state.catalog_path).map_err(|e| format!("open catalog: {e}"))?);
    }
    let cat = guard.as_mut().expect("catalog just inserted");
    let roots = watched_folders(cat);
    let emitter = app.clone();
    rebuild(cat, &roots, &move |p| {
        // Best-effort: a slow UI must never fail the rebuild itself.
        let _ = emitter.emit(REBUILD_EVENT, &p);
    })
    .inspect(|_report| {
        // Rebuilt rows may carry NULL hashes again (a fresh catalog does):
        // re-run the lazy hash backfill so the grid regains its thumb://
        // keys (PLAN §10 "hash lazily"). Best-effort, dedup-guarded.
        crate::spawn_hash_backfill(state.catalog_path.clone(), app.clone());
    })
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_identity() {
        assert_eq!(super::CRATE_NAME, "fotobase-rebuild");
    }
}
