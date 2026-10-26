//! Throttled scanning for network folders (TASK-02-009, PLAN §6 P1.5).
//!
//! Network shares need different scan behavior than local disks:
//!
//! - **IO budget** — per-folder-source ops/sec cap so browsing stays
//!   responsive while a scan runs (PLAN §3 rule 4: heavy work yields to the
//!   job queue; the pacing sleeps are what yield here).
//! - **Pause on disconnect** — an IO error flips the scan to a probe loop;
//!   if the cheap liveness probe fails the scan stops as
//!   [`NetworkScanState::Paused`], errors are aggregated into one count (no
//!   per-file error storm), and nothing is deleted (a paused scan must never
//!   mass-delete files it could not see).
//! - **Clean resume** — each directory is committed as its own transaction
//!   and recorded in `completed_dirs`; a resumed scan skips those
//!   directories entirely and continues from the last completed entry, so
//!   no work is duplicated.
//! - **Lazy hashing** — the scan never reads file contents, so
//!   `files.hash` stays NULL; [`backfill_hashes`](crate::backfill_hashes)
//!   fills hashes lazily (PLAN §10).
//!
//! Disconnects are simulated in tests via the injectable [`FsOps`] trait;
//! the real-SMB harness is TASK-02-010.

use std::collections::{HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fotobase_catalog::Catalog;

use crate::{media_type, ScanProgress};

/// One directory entry as the scan needs it: path, kind, size and mtime.
/// Content is never read — hashes stay lazy (PLAN §10).
#[derive(Debug, Clone)]
pub struct EntryMeta {
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: f64,
}

/// Filesystem operations the network scanner performs. Injectable so tests
/// can simulate disconnects and transient faults without a real share.
pub trait FsOps: Send + Sync {
    /// List `dir`'s children with their metadata. Per-entry errors are
    /// skipped by the implementor (aggregated upstream), not fatal.
    fn read_dir(&self, dir: &Path) -> io::Result<Vec<EntryMeta>>;
    /// Cheap liveness probe for the share (a statfs-class operation, not a
    /// directory walk). `false` means the root is unreachable.
    fn probe(&self, root: &Path) -> bool;
}

/// The real filesystem: one `read_dir` + per-entry metadata, and a
/// `metadata` stat as the liveness probe.
pub struct RealFs;

impl FsOps for RealFs {
    fn read_dir(&self, dir: &Path) -> io::Result<Vec<EntryMeta>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let Ok(entry) = entry else { continue };
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() && !ft.is_file() {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0.0, |d| d.as_secs_f64());
            out.push(EntryMeta {
                path: entry.path(),
                is_dir: ft.is_dir(),
                size: meta.len(),
                mtime,
            });
        }
        Ok(out)
    }

    fn probe(&self, root: &Path) -> bool {
        std::fs::metadata(root).is_ok()
    }
}

/// Tunables for [`scan_network_folder`].
#[derive(Clone)]
pub struct NetworkScanOptions {
    /// Filesystem operations implementation (injectable for tests).
    pub fs: Arc<dyn FsOps>,
    /// IO budget: max filesystem operations per second. `0` means
    /// unthrottled (tests; never for real shares).
    pub ops_per_sec: u32,
    /// How long to wait between liveness probes while recovering from an
    /// IO error.
    pub probe_interval: Duration,
    /// Retries (each preceded by a probe) before an IO error pauses the scan.
    pub max_retries: u32,
    /// Directories already committed by an earlier, paused run. The resumed
    /// scan skips them entirely — no duplicated work.
    pub completed_dirs: Vec<String>,
}

impl Default for NetworkScanOptions {
    fn default() -> Self {
        Self {
            fs: Arc::new(RealFs),
            ops_per_sec: 200,
            probe_interval: Duration::from_secs(1),
            max_retries: 3,
            completed_dirs: Vec::new(),
        }
    }
}

/// Terminal state of a network scan pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NetworkScanState {
    /// Every directory under the root was scanned.
    #[default]
    Completed,
    /// The share went away mid-scan; resume with `completed_dirs`.
    Paused,
    /// The caller's cancel flag stopped the scan.
    Cancelled,
}

/// What one network scan pass did. Errors are aggregated into `errors` —
/// a paused scan never surfaces a per-file error storm.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetworkScanReport {
    pub state: NetworkScanState,
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    pub removed: usize,
    /// Aggregated IO error count (not per-file error details).
    pub errors: usize,
    /// Directory paths committed so far (carried-over + this pass). Pass
    /// these back as `NetworkScanOptions::completed_dirs` to resume.
    pub completed_dirs: Vec<String>,
    /// Directory the scan paused at, if paused.
    pub paused_at: Option<String>,
}

impl NetworkScanReport {
    /// Did this scan change the catalog at all? (Mirrors
    /// [`crate::ScanReport::changed`].)
    pub fn changed(&self) -> bool {
        self.added + self.updated + self.removed > 0
    }
}

/// Ops-per-second rate limiter: a rolling 1-second window; once the budget
/// is used up the next op sleeps until the window rolls over. The sleeps
/// are how the scan loop yields to the rest of the system (PLAN §3 rule 4).
struct RateLimiter {
    budget: u32,
    window_start: Instant,
    used: u32,
}

impl RateLimiter {
    fn new(budget: u32) -> Self {
        Self { budget, window_start: Instant::now(), used: 0 }
    }

    fn acquire(&mut self) {
        if self.budget == 0 {
            return;
        }
        let elapsed = self.window_start.elapsed();
        if elapsed >= Duration::from_secs(1) {
            self.window_start = Instant::now();
            self.used = 0;
        } else if self.used >= self.budget {
            std::thread::sleep(Duration::from_secs(1) - elapsed);
            self.window_start = Instant::now();
            self.used = 0;
        }
        self.used += 1;
    }
}

/// Throttled, disconnect-tolerant incremental scan of a network folder.
///
/// Walks `root` breadth-first, one directory per catalog transaction:
/// seeds the folder row, upserts new/changed media files, deletes rows for
/// files that vanished from that directory. Completed directories are
/// recorded in the report so a paused scan can be resumed without redoing
/// work. On a disconnect (IO error + failed liveness probe, or retries
/// exhausted) the scan pauses: nothing is deleted, errors stay aggregated.
pub fn scan_network_folder(
    catalog: &mut Catalog,
    root: &Path,
    cancel: &AtomicBool,
    options: &NetworkScanOptions,
    progress: &dyn Fn(ScanProgress),
) -> fotobase_catalog::Result<NetworkScanReport> {
    let root_str = root.to_string_lossy().into_owned();
    let mut completed: Vec<String> = options.completed_dirs.clone();
    let completed_set: HashSet<String> = completed.iter().cloned().collect();
    let mut limiter = RateLimiter::new(options.ops_per_sec);
    let mut dirs_seen = 0usize;
    let mut files_seen = 0usize;
    let mut report = NetworkScanReport::default();

    if cancel.load(Ordering::Relaxed) {
        return Ok(NetworkScanReport {
            state: NetworkScanState::Cancelled,
            completed_dirs: completed,
            ..empty_report()
        });
    }

    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(root.to_path_buf());
    let mut seen_dirs: Vec<String> = vec![root_str.clone()];

    while let Some(dir) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(NetworkScanReport {
                state: NetworkScanState::Cancelled,
                completed_dirs: completed,
                ..empty_report()
            });
        }
        let dir_str = dir.to_string_lossy().into_owned();
        if completed_set.contains(&dir_str) {
            // Already committed by an earlier pass: no duplicated file work.
            // Still list the directory (cheap, throttled) so subdirectories
            // added during the pause are discovered too.
            limiter.acquire();
            match read_dir_resilient(options, root, &dir) {
                Ok(entries) => {
                    for entry in entries.iter().filter(|e| e.is_dir) {
                        seen_dirs.push(entry.path.to_string_lossy().into_owned());
                        queue.push_back(entry.path.clone());
                    }
                }
                Err(_) => {
                    return Ok(NetworkScanReport {
                        state: NetworkScanState::Paused,
                        errors: report.errors + 1,
                        paused_at: Some(dir_str),
                        completed_dirs: completed,
                        ..report
                    });
                }
            }
            continue;
        }

        limiter.acquire();
        let entries = match read_dir_resilient(options, root, &dir) {
            Ok(entries) => entries,
            Err(_) => {
                // Disconnected: aggregate the error, record where we stopped,
                // and never delete anything on the way out.
                return Ok(NetworkScanReport {
                    state: NetworkScanState::Paused,
                    errors: report.errors + 1,
                    paused_at: Some(dir_str),
                    completed_dirs: completed,
                    ..report
                });
            }
        };
        dirs_seen += 1;
        progress(ScanProgress { dirs_seen, files_seen });

        catalog.begin_uncommitted_batch()?;
        let outcome = process_directory(catalog, &dir, &entries, &mut report, &mut limiter);
        match outcome {
            Ok(()) => {
                catalog.commit_batch()?;
                completed.push(dir_str.clone());
            }
            Err(_) => {
                let _ = catalog.conn().execute_batch("ROLLBACK");
                return Ok(NetworkScanReport {
                    state: NetworkScanState::Paused,
                    errors: report.errors + 1,
                    paused_at: Some(dir_str),
                    completed_dirs: completed,
                    ..report
                });
            }
        }
        files_seen += entries.iter().filter(|e| !e.is_dir).count();
        progress(ScanProgress { dirs_seen, files_seen });

        for entry in entries.iter().filter(|e| e.is_dir) {
            seen_dirs.push(entry.path.to_string_lossy().into_owned());
            queue.push_back(entry.path.clone());
        }
    }

    // Fully completed: prune folder rows (and their cascaded files) for
    // directories that vanished from the enumeration — safe now, because
    // every surviving directory was listed.
    let sep = std::path::MAIN_SEPARATOR.to_string();
    let folders = catalog_folders_under(catalog, &root_str, &sep)?;
    for path in folders {
        if path != root_str && !seen_dirs.iter().any(|d| d == &path) {
            catalog.conn().execute("DELETE FROM folders WHERE path = ?1", [path.as_str()])?;
        }
    }
    catalog.conn().execute(
        "UPDATE folders SET last_scan_at = datetime('now') WHERE path = ?1",
        [&root_str],
    )?;

    Ok(NetworkScanReport {
        state: NetworkScanState::Completed,
        completed_dirs: completed,
        ..report
    })
}

fn empty_report() -> NetworkScanReport {
    NetworkScanReport::default()
}

/// Retry an IO error while the share is still alive: probe, wait, retry,
/// up to `max_retries`. The first failed probe (or exhausted retries)
/// surfaces the error so the scan pauses.
fn read_dir_resilient(
    options: &NetworkScanOptions,
    root: &Path,
    dir: &Path,
) -> io::Result<Vec<EntryMeta>> {
    match options.fs.read_dir(dir) {
        Ok(entries) => Ok(entries),
        Err(first) => {
            for _ in 0..options.max_retries {
                std::thread::sleep(options.probe_interval);
                if !options.fs.probe(root) {
                    return Err(first);
                }
                match options.fs.read_dir(dir) {
                    Ok(entries) => return Ok(entries),
                    Err(_) => continue,
                }
            }
            // Still failing with a live-looking share: treat as a disconnect
            // too — the spec's rule is "IO errors flip the scan to paused".
            let _ = first;
            Err(io::Error::new(io::ErrorKind::TimedOut, "share unresponsive"))
        }
    }
}

/// Upsert one directory's files inside the caller's transaction: seed the
/// folder, compare (mtime, size) against existing rows for that folder,
/// upsert new/changed files, delete rows for files that vanished. File
/// counting paces through the limiter like every other op.
fn process_directory(
    catalog: &mut Catalog,
    dir: &Path,
    entries: &[EntryMeta],
    report: &mut NetworkScanReport,
    limiter: &mut RateLimiter,
) -> fotobase_catalog::Result<()> {
    let dir_str = dir.to_string_lossy().into_owned();
    let folder_id = catalog.seed_folder(&dir_str)?;

    // Existing rows for this folder only — cheap per-directory compare.
    let mut stmt =
        catalog.conn().prepare("SELECT path, size, mtime FROM files WHERE folder_id = ?1")?;
    let existing: std::collections::HashMap<String, (i64, f64)> = stmt
        .query_map([folder_id], |r| {
            Ok((r.get::<_, String>(0)?, (r.get::<_, i64>(1)?, r.get::<_, f64>(2)?)))
        })?
        .flatten()
        .collect();
    drop(stmt);

    let mut walked: HashSet<String> = HashSet::new();
    for entry in entries.iter().filter(|e| !e.is_dir) {
        limiter.acquire();
        let path_str = entry.path.to_string_lossy().into_owned();
        let Some(media) = media_type(&entry.path) else { continue };
        walked.insert(path_str.clone());
        match existing.get(&path_str) {
            Some((size, mtime)) if *size == entry.size as i64 && *mtime == entry.mtime => {
                report.skipped += 1;
            }
            Some(_) => {
                upsert(catalog, folder_id, &path_str, entry, media)?;
                report.updated += 1;
            }
            None => {
                upsert(catalog, folder_id, &path_str, entry, media)?;
                report.added += 1;
            }
        }
    }

    for path in existing.keys() {
        if !walked.contains(path) {
            catalog.conn().execute("DELETE FROM files WHERE path = ?1", [path.as_str()])?;
            report.removed += 1;
        }
    }
    Ok(())
}

fn upsert(
    catalog: &Catalog,
    folder_id: i64,
    path: &str,
    entry: &EntryMeta,
    media: &str,
) -> fotobase_catalog::Result<()> {
    let filename =
        entry.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    catalog.conn().execute(
        "INSERT INTO files(folder_id, path, filename, size, mtime, media_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(path) DO UPDATE SET
            folder_id = excluded.folder_id,
            filename = excluded.filename,
            size = excluded.size,
            mtime = excluded.mtime,
            media_type = excluded.media_type",
        rusqlite::params![folder_id, path, filename, entry.size as i64, entry.mtime, media],
    )?;
    Ok(())
}

fn catalog_folders_under(
    catalog: &Catalog,
    root_str: &str,
    sep: &str,
) -> fotobase_catalog::Result<Vec<String>> {
    let mut stmt =
        catalog.conn().prepare("SELECT path FROM folders WHERE path LIKE ?1 || ?2 || '%'")?;
    let rows = stmt.query_map(rusqlite::params![root_str, sep], |r| r.get::<_, String>(0))?;
    Ok(rows.flatten().collect())
}
