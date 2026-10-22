//! File-menu / folder-context commands (TASK-01-021, PLAN §2).
//!
//! "Locate on disk" reveals a file or folder in the OS file manager; "Delete
//! from disk" moves one photo to the OS trash — never a permanent delete —
//! and drops its catalog row; "Refresh thumbnails" invalidates a folder's
//! cache entries and re-queues their renders through the bounded job queue
//! at background priority (PLAN §3 rule 4).
//!
//! As in `thumb.rs` / `rebuild.rs`, the pure, app-free core lives here so it
//! is testable without a Tauri runtime; `lib.rs` holds the thin `#[tauri::command]`
//! adapters. The confirmation dialogs are the UI's job (frontend).

use std::path::{Path, PathBuf};

use fotobase_catalog::Catalog;
use fotobase_jobs::{Priority, Queue};
use fotobase_render::thumbnail_job;

use crate::thumb::ThumbOutcome;

/// "Locate on disk" (PLAN §2 File menu / Folder context menu): reveal a file
/// or folder in the OS file manager (Finder / Explorer / the freedesktop
/// handler), with the item selected. Works for folders too.
pub fn reveal_path(path: &str) -> Result<(), String> {
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| format!("reveal {path}: {e}"))
}

/// "Delete from disk" (PLAN §2, §3 rule 1): move the photo to the OS trash
/// (Recycle Bin / Trash / freedesktop spec) and only then drop its catalog
/// row. Order matters: a failed trash leaves both the file and the row
/// untouched — the library must never claim a photo it cannot show.
/// Confirmation happens in the UI before this runs; there is no permanent
/// delete anywhere on this path.
pub fn delete_to_trash(file_id: i64, path: &str, catalog: &mut Catalog) -> Result<(), String> {
    trash::delete(path).map_err(|e| format!("move to trash {path}: {e}"))?;
    catalog
        .remove_file(file_id)
        .map(|_| ())
        .map_err(|e| format!("drop catalog row for {path}: {e}"))
}

/// "Refresh thumbnails" (PLAN §2 Folder context menu): delete the folder's
/// cached thumbnail entries, then re-queue a render job per cataloged
/// content hash at [`Priority::Background`] — the user asked for a refresh,
/// not for jumping the visible-first queue (PLAN §3 rule 4).
///
/// Jobs are keyed by hash (they coalesce, matching the cache-miss pipeline),
/// and the cache entries are removed *before* enqueuing: [`thumbnail_job`]
/// short-circuits to the cached file when one exists, so a re-queue without
/// invalidation would be a no-op. Returns how many jobs were enqueued; an
/// unknown folder re-queues nothing.
pub fn refresh_folder_thumbnails(
    catalog: &Catalog,
    cache_dir: &Path,
    queue: &Queue<ThumbOutcome>,
    path: &str,
) -> Result<usize, String> {
    let folder = catalog
        .list_folders()
        .map_err(|e| format!("list folders: {e}"))?
        .into_iter()
        .find(|f| f.path == path);
    let Some(folder) = folder else { return Ok(0) };

    let hashes: Vec<String> = catalog
        .list_files_in_folders(&[folder.id], usize::MAX)
        .map_err(|e| format!("list files in {path}: {e}"))?
        .into_iter()
        .filter_map(|file| file.hash)
        .filter(|hash| crate::thumb::is_cache_key(hash))
        .collect();

    let mut enqueued = 0;
    for hash in hashes {
        // The hash is the content hash, so the entry is shared by every
        // copy of that content — invalidating it once covers them all, and
        // the re-render regenerates it for everyone.
        let _ = std::fs::remove_file(cache_dir.join(format!("{hash}.jpg")));
        let _ = std::fs::remove_file(cache_dir.join(format!("{hash}.jpg.tmp")));
        for src in catalog.paths_for_hash(&hash).map_err(|e| format!("resolve {hash}: {e}"))? {
            if queue
                .try_enqueue(
                    hash.clone(),
                    Priority::Background,
                    thumbnail_job(PathBuf::from(&src), cache_dir.to_path_buf(), Default::default()),
                )
                .is_ok()
            {
                enqueued += 1;
            }
            // A full queue is the correct degradation (§3 rule 4): the next
            // `thumb://` miss re-triggers the render, like the cache-miss
            // pipeline does.
        }
    }
    Ok(enqueued)
}
