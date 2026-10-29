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
use fotobase_video::VideoMeta as ProbeMeta;

pub mod batch;
mod polling;
pub mod reassoc;
pub use polling::{
    is_network_path, AdaptiveInterval, PollConfig, Poller, DEFAULT_FAST_THRESHOLD,
    DEFAULT_POLL_MAX, DEFAULT_POLL_MIN, DEFAULT_SLOW_THRESHOLD,
};

mod network;
pub use network::{
    scan_network_folder, EntryMeta, FsOps, NetworkScanOptions, NetworkScanReport, NetworkScanState,
    RealFs,
};

mod watcher;
pub use watcher::{ScanHook, Watcher, WatcherError, DEFAULT_DEBOUNCE};

/// Video probe integration (TASK-02-003): the scanner calls ffprobe (via
/// [`fotobase_video`]) for `media_type = video` files at scan time.
pub use fotobase_video::{Ffprobe as DefaultVideoProbe, VideoError, VideoProbe};

pub use fotobase_catalog::MissingFile;
pub use reassoc::{follow_sidecar, match_missing, try_reassociate, ReassocOutcome};

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
    /// New-looking paths re-associated with vanished files by
    /// (size, mtime, hash) matching (TASK-03-003, 00-010 §4). Not counted in
    /// `added`: the file is not new, its user data was restored.
    pub reassociated: usize,
    /// Byte-identical candidates that could not be told apart — surfaced,
    /// never silently resolved (00-010 §4.4). The file is added as new.
    pub ambiguous: usize,
    /// True if the cancel flag stopped the walk early.
    pub cancelled: bool,
    /// Folders whose `Picasa.ini` this scan met for the first time: the
    /// migration is *offered* (TASK-03-022), never applied without the
    /// user's answer. Not part of [`ScanReport::changed`]: nothing in the
    /// library changed.
    pub picasa_offered: Vec<String>,
}

impl ScanReport {
    /// Did this scan change the catalog at all? No-op scans (everything
    /// matched by mtime+size) must not wake the UI — the `library-changed`
    /// event only fires for real changes.
    pub fn changed(&self) -> bool {
        self.added + self.updated + self.removed + self.reassociated > 0
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
    scan_folder_with_probes(catalog, root, cancel, options, progress, &DefaultVideoProbe)
}

/// [`scan_folder_with`] with an injected video probe. The default scan uses
/// real ffprobe ([`Ffprobe`]); tests inject fixtures and failure modes. The
/// probe runs inside the scan transaction: every added/updated video file is
/// probed, successful results land in `metadata`, and a failed probe is
/// logged and *marked* (`video_probe_ok = 0`) — a corrupt video never fails
/// the scan (PLAN §9).
pub fn scan_folder_with_probes(
    catalog: &mut Catalog,
    root: &Path,
    cancel: &AtomicBool,
    options: &ScanOptions,
    progress: &dyn Fn(ScanProgress),
    video_probe: &dyn VideoProbe,
) -> Result<ScanReport> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(ScanReport { cancelled: true, ..ScanReport::default() });
    }

    // Collect the walk first so the catalog transaction stays short.
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let mut cancelled = false;
    let mut picasa_dirs = Vec::new();
    walk_dir(root, &mut dirs, &mut files, &mut picasa_dirs, cancel, &mut cancelled, progress);
    // Per-directory callbacks fire on entry, before that directory's files
    // are pushed; close with the final walk totals so the last report is
    // complete.
    if !cancelled {
        progress(ScanProgress { dirs_seen: dirs.len(), files_seen: files.len() });
    }

    catalog.begin_uncommitted_batch()?;
    let mut touched: Vec<String> = Vec::new();
    let result = scan_inner(catalog, root, &dirs, &files, options.full, &mut touched)
        .and_then(|report| probe_videos(catalog, root, cancel, video_probe).map(|_| report));
    match result {
        Ok(report) => {
            catalog.commit_batch()?;
            // Sidecars are the source of truth for user data (PLAN §5): a
            // fresh or rebuilt catalog picks stars/tags back up from them.
            catalog.sync_paths(touched.iter().map(String::as_str));
            let picasa_offered = offer_picasa_ingest(catalog, &picasa_dirs);
            Ok(ScanReport { cancelled, picasa_offered, ..report })
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
    touched: &mut Vec<String>,
) -> Result<ScanReport> {
    let root_str = root.to_string_lossy().into_owned();
    catalog.seed_folder(&root_str)?;
    for dir in dirs {
        catalog.seed_folder(&dir.to_string_lossy())?;
    }
    // Folder-level hide (`.fotobase`) is inherited at scan time (ADR-0007).
    let dir_strs: Vec<String> = dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect();
    if let Err(err) = catalog.sync_folder_flags(
        std::iter::once(root_str.as_str()).chain(dir_strs.iter().map(String::as_str)),
    ) {
        tracing::warn!(root = %root_str, error = %err, "syncing folder hide flags failed");
    }

    // Existing rows under the root, keyed by path for cheap (mtime, size) compare.
    let existing = existing_files(catalog, &root_str)?;

    let mut report = ScanReport::default();

    // Snapshot vanished files and delete them first, so the same scan can
    // already re-associate the moved/renamed file that replaced them
    // (00-010 §4: matching happens against the missing-file set).
    let walked: HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let vanished: Vec<String> =
        existing.keys().filter(|p| !walked.contains(p.as_str())).cloned().collect();
    if !vanished.is_empty() {
        catalog.snapshot_missing(&vanished)?;
    }
    for path in &vanished {
        catalog.conn().execute("DELETE FROM files WHERE path = ?1", [path.as_str()])?;
        report.removed += 1;
    }

    for f in files {
        match existing.get(f.path.as_str()) {
            Some((size, mtime)) if !full && *size == f.size && *mtime == f.mtime => {
                report.skipped += 1
            }
            Some(_) => {
                upsert_file(catalog, f)?;
                touched.push(f.path.clone());
                report.updated += 1;
            }
            None => {
                // The old file may be back at its original location (moved
                // away and returned): forget the missing record either way.
                catalog.drop_missing(&f.path)?;
                match try_reassociate(catalog, &f.path, &f.dir, f.size, f.mtime, f.media_type)? {
                    ReassocOutcome::Match(_) => report.reassociated += 1,
                    ReassocOutcome::Ambiguous(_) => {
                        report.ambiguous += 1;
                        upsert_file(catalog, f)?;
                        touched.push(f.path.clone());
                        report.added += 1;
                    }
                    ReassocOutcome::NoMatch => {
                        upsert_file(catalog, f)?;
                        touched.push(f.path.clone());
                        report.added += 1;
                    }
                }
            }
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

/// Record the folders whose Picasa.ini this scan met (TASK-03-022). Only
/// a folder never seen before is newly offered; a declined or migrated one
/// stays quiet. A failure to record never fails the scan.
fn offer_picasa_ingest(catalog: &mut Catalog, dirs: &[PathBuf]) -> Vec<String> {
    let mut offered = Vec::new();
    for dir in dirs {
        let dir = dir.to_string_lossy();
        match catalog.offer_picasa_ingest(&dir) {
            Ok(true) => offered.push(dir.into_owned()),
            Ok(false) => {}
            Err(err) => {
                tracing::warn!(dir = %dir, error = %err, "recording Picasa.ini offer failed")
            }
        }
    }
    offered
}

/// `picasa_dirs` collects the directories holding a Picasa.ini.
fn walk_dir(
    dir: &Path,
    dirs: &mut Vec<PathBuf>,
    files: &mut Vec<WalkedFile>,
    picasa_dirs: &mut Vec<PathBuf>,
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
    let mut has_picasa_ini = false;
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
        if name.to_str().is_some_and(fotobase_metadata::is_picasa_ini_name) {
            has_picasa_ini = true;
            continue;
        }
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

    if has_picasa_ini {
        picasa_dirs.push(dir.to_path_buf());
    }

    for sub in subdirs {
        walk_dir(&sub, dirs, files, picasa_dirs, cancel, cancelled, progress);
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

/// Probe the videos this scan touched. Runs inside the scan transaction:
/// each added/updated video gets one ffprobe call, a successful result is
/// stored (`catalog::set_video_metadata`) and a failure is logged with the
/// file marked (`mark_video_probe_failed`) — corrupt media never aborts the
/// scan (PLAN §9: malformed files must never crash).
fn probe_videos(
    catalog: &mut Catalog,
    root: &Path,
    cancel: &AtomicBool,
    video_probe: &dyn VideoProbe,
) -> Result<()> {
    let root_str = root.to_string_lossy().into_owned();
    let pending = catalog.videos_needing_probe_under(&root_str, PROBE_BATCH)?;
    for (file_id, path, mtime) in pending {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        match video_probe.probe(Path::new(&path)) {
            Ok(meta) => {
                catalog.set_video_metadata(file_id, &to_catalog_meta(&meta), mtime)?;
            }
            Err(e) => {
                tracing::warn!(file = %path, error = %e, "video probe failed; file marked");
                catalog.mark_video_probe_failed(file_id, mtime)?;
            }
        }
    }
    Ok(())
}

/// Per-scan probe batch: a scan never grows unboundedly (§3 rule 4). A
/// library with more unprobed videos than this finishes via
/// [`refresh_video_metadata`] passes driven by the job queue.
const PROBE_BATCH: usize = 4096;

fn to_catalog_meta(meta: &ProbeMeta) -> fotobase_catalog::VideoMeta {
    fotobase_catalog::VideoMeta {
        duration_secs: meta.duration_secs,
        codec: meta.codec.clone(),
        profile: meta.profile.clone(),
        rotation_deg: meta.rotation_deg,
    }
}

/// Bounded standalone pass over every catalogued video still needing a probe
/// (never-probed, or probed at a stale mtime). The app drives this after a
/// scan that changed something and on schema upgrade, so a library scanned
/// before this feature gains its video metadata without a full rescan.
/// Returns how many videos were probed this pass; `0` means done, the
/// caller's stop signal — same contract as [`backfill_hashes`]. Probe
/// failures are logged and marked, never fatal.
pub fn refresh_video_metadata(
    catalog: &mut Catalog,
    batch: usize,
    cancel: &AtomicBool,
    video_probe: &dyn VideoProbe,
) -> Result<usize> {
    let pending = catalog.videos_needing_probe(batch)?;
    let mut probed = 0usize;
    for (file_id, path, mtime) in pending {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        match video_probe.probe(Path::new(&path)) {
            Ok(meta) => {
                catalog.set_video_metadata(file_id, &to_catalog_meta(&meta), mtime)?;
                probed += 1;
            }
            Err(e) => {
                tracing::warn!(file = %path, error = %e, "video probe failed; file marked");
                catalog.mark_video_probe_failed(file_id, mtime)?;
            }
        }
    }
    Ok(probed)
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
