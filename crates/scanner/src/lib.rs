//! Folder walk, `notify` watcher, import (PLAN §3).
//!
//! Incremental scanning per PLAN §5: a scan walks a watched folder, upserts
//! files whose (mtime, size) changed, and removes catalog rows for files that
//! vanished. Content hashes are computed lazily elsewhere (§10: cache by
//! mtime+size, hash lazily) — the scanner never reads file contents, so an
//! unchanged-library rescan performs zero file re-reads.
//!
//! Heavy scan work must stay bounded and cancellable (§3 rule 4): the caller
//! passes an [`AtomicBool`] cancel flag and the walk checks it throughout.
//! The job queue (TASK-01-003) will drive this API once it exists.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use fotobase_catalog::{Catalog, Result};

mod polling;
pub use polling::{
    is_network_path, AdaptiveInterval, PollConfig, Poller, DEFAULT_FAST_THRESHOLD,
    DEFAULT_POLL_MAX, DEFAULT_POLL_MIN, DEFAULT_SLOW_THRESHOLD,
};

mod watcher;
pub use watcher::{ScanHook, Watcher, WatcherError, DEFAULT_DEBOUNCE};

pub mod import;
pub use import::{
    content_hash, copy_file, import_folder, ImportOptions, ImportProgress, ImportReport, Naming,
};

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-scanner";

/// What one scan did. Skipped files were matched by (mtime, size) without
/// being re-read.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScanReport {
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    pub removed: usize,
    /// True if the cancel flag stopped the walk early.
    pub cancelled: bool,
}

impl ScanReport {
    /// Did this scan change the catalog at all? No-op scans (everything
    /// matched by mtime+size) must not wake the UI — the `library-changed`
    /// event only fires for real changes.
    pub fn changed(&self) -> bool {
        self.added + self.updated + self.removed > 0
    }
}

/// One bounded pass of the lazy content-hash backfill (PLAN §10 "hash
/// lazily"): fills up to `batch` of the catalog's `hash IS NULL` rows by
/// hashing the file on disk (`content_hash`, the render cache-key
/// convention) and storing the result.
///
/// A scan never reads file contents, so a freshly scanned library is a
///catalog full of NULL hashes and the grid has no `thumb://` keys — the
/// app drives this pass (batch after batch, on its own connection) after a
/// scan that changed something. Returns how many rows were hashed this
/// pass; `0` means done (or nothing hashable), which is the caller's stop
/// signal. A vanished/unreadable file stays unhashed and reports no
/// progress, so a permanently broken row cannot spin the loop. `cancel`
/// stops at the next file, like every other scanner entry point.
pub fn backfill_hashes(catalog: &mut Catalog, batch: usize, cancel: &AtomicBool) -> Result<usize> {
    let rows = catalog.unhashed_files(batch)?;
    let mut hashed = 0usize;
    for (id, path) in rows {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if let Ok(hash) = content_hash(Path::new(&path)) {
            catalog.set_file_hash(id, &hash)?;
            hashed += 1;
        }
    }
    Ok(hashed)
}

/// Walk position reported through the [`scan_folder_with`] progress
/// callback (TASK-01-023). Counts are cumulative for the current scan call.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScanProgress {
    pub dirs_seen: usize,
    pub files_seen: usize,
}

/// Scan behavior knobs.
///
/// Default is the incremental scan (PLAN §5): files whose (mtime, size) is
/// unchanged are skipped without being re-read. `full: true` disables that
/// skip — the explicit, progress-reported rebuild path (PLAN §5: "a full
/// rebuild is an explicit, progress-reported maintenance action"); every
/// media file is re-upserted so derived columns are repopulated.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScanOptions {
    pub full: bool,
}

/// Extensions the scanner catalogues. Everything else (sidecars, txt…) is
/// ignored; media_type is `image` or `video` per the catalog schema.
const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "tif", "tiff", "webp", "heic", "heif", "avif", "bmp", "dng",
    "cr2", "cr3", "nef", "arw", "raf", "orf", "rw2",
];
const VIDEO_EXTS: &[&str] = &["mp4", "mov", "m4v", "avi", "mkv", "webm", "mts"];

fn media_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    if IMAGE_EXTS.contains(&ext.as_ref()) {
        Some("image")
    } else if VIDEO_EXTS.contains(&ext.as_ref()) {
        Some("video")
    } else {
        None
    }
}

struct WalkedFile {
    path: String,
    dir: String,
    filename: String,
    size: i64,
    mtime: f64,
    media_type: &'static str,
}

/// Incrementally scan `root` into the catalog.
///
/// Walks `root` recursively, seeds a `folders` row per directory, upserts
/// media files that are new or whose (mtime, size) changed, deletes rows for
/// files and directories that vanished, and stamps `last_scan_at` on the root.
/// All writes happen in one transaction: a crash mid-scan rolls back cleanly.
pub fn scan_folder(catalog: &mut Catalog, root: &Path, cancel: &AtomicBool) -> Result<ScanReport> {
    scan_folder_with(catalog, root, cancel, &ScanOptions::default(), &|_| {})
}

/// [`scan_folder`] with explicit options and a progress callback. The
/// callback fires once per directory entered, with cumulative counts.
pub fn scan_folder_with(
    catalog: &mut Catalog,
    root: &Path,
    cancel: &AtomicBool,
    options: &ScanOptions,
    progress: &dyn Fn(ScanProgress),
) -> Result<ScanReport> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(ScanReport { cancelled: true, ..ScanReport::default() });
    }

    // Collect the walk first so the catalog transaction stays short.
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let mut cancelled = false;
    walk_dir(root, &mut dirs, &mut files, cancel, &mut cancelled, progress);
    // Per-directory callbacks fire on entry, before that directory's files
    // are pushed; close with the final walk totals so the last report is
    // complete.
    if !cancelled {
        progress(ScanProgress { dirs_seen: dirs.len(), files_seen: files.len() });
    }

    catalog.begin_uncommitted_batch()?;
    let result = scan_inner(catalog, root, &dirs, &files, options.full);
    match result {
        Ok(report) => {
            catalog.commit_batch()?;
            Ok(ScanReport { cancelled, ..report })
        }
        Err(e) => {
            let _ = catalog.conn().execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

fn scan_inner(
    catalog: &mut Catalog,
    root: &Path,
    dirs: &[PathBuf],
    files: &[WalkedFile],
    full: bool,
) -> Result<ScanReport> {
    let root_str = root.to_string_lossy().into_owned();
    catalog.seed_folder(&root_str)?;
    for dir in dirs {
        catalog.seed_folder(&dir.to_string_lossy())?;
    }

    // Existing rows under the root, keyed by path for cheap (mtime, size) compare.
    let existing = existing_files(catalog, &root_str)?;

    let mut report = ScanReport::default();
    let walked: HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();

    for f in files {
        match existing.get(f.path.as_str()) {
            Some((size, mtime)) if !full && *size == f.size && *mtime == f.mtime => {
                report.skipped += 1
            }
            Some(_) => {
                upsert_file(catalog, f)?;
                report.updated += 1;
            }
            None => {
                upsert_file(catalog, f)?;
                report.added += 1;
            }
        }
    }

    for path in existing.keys() {
        if !walked.contains(path.as_str()) {
            catalog.conn().execute("DELETE FROM files WHERE path = ?1", [path.as_str()])?;
            report.removed += 1;
        }
    }

    // Prune folder rows for directories that no longer exist on disk (their
    // files cascade away; any surviving files were deleted explicitly above).
    for path in existing_folders(catalog, &root_str)? {
        if !Path::new(&path).exists() {
            catalog.conn().execute("DELETE FROM folders WHERE path = ?1", [path.as_str()])?;
        }
    }

    catalog.conn().execute(
        "UPDATE folders SET last_scan_at = datetime('now') WHERE path = ?1",
        [&root_str],
    )?;
    Ok(report)
}

fn walk_dir(
    dir: &Path,
    dirs: &mut Vec<PathBuf>,
    files: &mut Vec<WalkedFile>,
    cancel: &AtomicBool,
    cancelled: &mut bool,
    progress: &dyn Fn(ScanProgress),
) {
    if *cancelled || cancel.load(Ordering::Relaxed) {
        *cancelled = true;
        return;
    }
    dirs.push(dir.to_path_buf());
    progress(ScanProgress { dirs_seen: dirs.len(), files_seen: files.len() });

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        // Unreadable directory: skip it, don't fail the whole scan.
        Err(e) => {
            tracing::warn!(dir = %dir.display(), error = %e, "skipping unreadable directory");
            return;
        }
    };

    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            subdirs.push(path);
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let Some(name) = path.file_name() else { continue };
        let Some(media) = media_type(&path) else { continue };
        let Ok(meta) = entry.metadata() else { continue };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0.0, |d| d.as_secs_f64());
        files.push(WalkedFile {
            path: path.to_string_lossy().into_owned(),
            dir: dir.to_string_lossy().into_owned(),
            filename: name.to_string_lossy().into_owned(),
            size: meta.len() as i64,
            mtime,
            media_type: media,
        });
    }

    for sub in subdirs {
        walk_dir(&sub, dirs, files, cancel, cancelled, progress);
    }
}

fn existing_files(catalog: &Catalog, root_str: &str) -> Result<HashMap<String, (i64, f64)>> {
    // Separator comes in as a parameter: Windows paths use `\`, so a
    // hardcoded `LIKE ?1 || '/%'` would never match subdirectories there and
    // every rescan would re-add the whole tree.
    let sep = std::path::MAIN_SEPARATOR.to_string();
    let mut stmt = catalog.conn().prepare(
        "SELECT f.path, f.size, f.mtime FROM files f
         JOIN folders d ON d.id = f.folder_id
         WHERE d.path = ?1 OR d.path LIKE ?1 || ?2 || '%'",
    )?;
    let rows = stmt.query_map(rusqlite::params![root_str, sep], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (path, size, mtime) = row?;
        map.insert(path, (size, mtime));
    }
    Ok(map)
}

fn existing_folders(catalog: &Catalog, root_str: &str) -> Result<Vec<String>> {
    let sep = std::path::MAIN_SEPARATOR.to_string();
    let mut stmt = catalog
        .conn()
        .prepare("SELECT path FROM folders WHERE path != ?1 AND path LIKE ?1 || ?2 || '%'")?;
    let rows = stmt.query_map(rusqlite::params![root_str, sep], |r| r.get::<_, String>(0))?;
    Ok(rows.flatten().collect())
}

fn upsert_file(catalog: &Catalog, f: &WalkedFile) -> Result<()> {
    let folder_id: i64 =
        catalog
            .conn()
            .query_row("SELECT id FROM folders WHERE path = ?1", [&f.dir], |r| r.get(0))?;
    catalog.conn().execute(
        "INSERT INTO files(folder_id, path, filename, size, mtime, media_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(path) DO UPDATE SET
            folder_id = excluded.folder_id,
            filename = excluded.filename,
            size = excluded.size,
            mtime = excluded.mtime,
            media_type = excluded.media_type",
        rusqlite::params![folder_id, f.path, f.filename, f.size, f.mtime, f.media_type],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_identity() {
        assert_eq!(super::CRATE_NAME, "fotobase-scanner");
    }
}
