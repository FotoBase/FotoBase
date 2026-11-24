//! Camera roll as a watched-folder equivalent (TASK-09-006, PLAN §6 Phase 5).
//!
//! The native observers (`tauri-plugin-camera-roll`) report which folders of
//! camera roll media changed. This module turns such a report into the scan
//! requests the folder watcher already understands: a change inside a watched
//! root is a rescan request for that root — exactly what a filesystem event
//! produces — and a folder nobody watches yet becomes a watched root. There is
//! no scanner of its own.

use std::path::{Path, PathBuf};

use fotobase_catalog::is_within;

/// One step the folder manager runs for a camera roll change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanRequest {
    /// Start watching this folder (Scan Always); its closing scan catalogs it.
    Watch(PathBuf),
    /// Incrementally rescan this watched root.
    Rescan(PathBuf),
}

fn covers(root: &Path, folder: &Path) -> bool {
    let (root, folder) = (root.to_string_lossy(), folder.to_string_lossy());
    folder == root || is_within(&folder, &root)
}

/// Map the folders of a change onto scan requests, in first-seen order.
///
/// A folder inside a watched root (or inside a folder just chosen to be
/// watched) folds into that root: watching the root covers its tree, and a
/// nested root would double-scan. Each root appears once.
pub fn plan_scans(watched: &[PathBuf], folders: &[PathBuf]) -> Vec<ScanRequest> {
    let mut plan: Vec<ScanRequest> = Vec::new();
    for folder in folders {
        if let Some(root) = watched.iter().find(|r| covers(r, folder)) {
            let request = ScanRequest::Rescan(root.clone());
            if !plan.contains(&request) {
                plan.push(request);
            }
        } else if !plan.iter().any(|r| matches!(r, ScanRequest::Watch(w) if covers(w, folder))) {
            plan.push(ScanRequest::Watch(folder.clone()));
        }
    }
    plan
}
