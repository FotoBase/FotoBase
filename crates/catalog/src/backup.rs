//! Backup Manager engine (TASK-08-014, PLAN §2L "Backup Pictures", §6 P4
//! "Incremental backup + restore"): incremental backup sets of the catalog,
//! the XMP sidecars and the settings files to an external location.
//!
//! Sidecars and settings are the user's data (PLAN §5 authority split); the
//! catalog rides along as a consistent `VACUUM INTO` snapshot so a restore
//! does not need a full rebuild. Originals are never read or written here.
//!
//! # Set layout (format v1)
//!
//! ```text
//! <destination>/fotobase-backup/
//!   <set-id>/manifest.json        commit marker + full file list
//!   <set-id>/files/<sha256>       content-addressed blobs new in this set
//!   <set-id>.partial/             a run in progress (never a set)
//! ```
//!
//! Every manifest lists *every* backed-up file (kind, original path, size,
//! mtime, SHA-256) and the set whose `files/` holds its blob, so any set is
//! restorable from the destination alone. The first set is full; later sets
//! copy only files whose (size, mtime) changed since the previous set — the
//! PLAN §5 incremental-rebuild skip — and whose content hash is not already
//! stored, referencing earlier sets for the rest.
//!
//! # Crash / unplug safety
//!
//! Blobs are staged in `<set-id>.partial/`, the directory is renamed to
//! `<set-id>`, and only then is `manifest.json` written (`.tmp` + rename).
//! A set without a manifest is not a set: a full or unplugged destination,
//! a cancel, or a crash never leaves a manifest describing missing blobs.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{civil_from_days, sidecar_path, Catalog, CatalogError};

/// Folder created under the chosen destination; holds every set.
pub const BACKUP_DIR: &str = "fotobase-backup";
/// The per-set manifest: its presence is what makes a directory a set.
pub const MANIFEST_FILE: &str = "manifest.json";
/// `format` tag of a v1 manifest.
pub const MANIFEST_FORMAT: &str = "fotobase-backup-set";
pub const MANIFEST_VERSION: u32 = 1;

const PARTIAL_SUFFIX: &str = ".partial";
pub(crate) const FILES_DIR: &str = "files";
const TMP_BLOB: &str = ".incoming";
const PROBE_FILE: &str = ".fb-write-probe";

/// What a backed-up file is (drives where a restore puts it back).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryKind {
    /// The SQLite catalog database (a consistent snapshot).
    Catalog,
    /// An XMP sidecar or folder flag file (`.fotobase`).
    Sidecar,
    /// App or library settings (`settings.json`, `albums.json`, …).
    Settings,
}

/// One file to back up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupSource {
    pub kind: EntryKind,
    pub path: PathBuf,
}

/// One file as recorded in a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub kind: EntryKind,
    /// Original location, as the library spells it.
    pub path: String,
    pub size: u64,
    pub mtime_secs: i64,
    pub mtime_nanos: u32,
    /// Lowercase hex SHA-256 of the content; also the blob's file name.
    pub sha256: String,
    /// Set whose `files/` directory holds the blob.
    pub set: String,
}

/// A set's `manifest.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    /// The set's directory name.
    pub id: String,
    /// Position in the destination's chain (1 = first set); orders sets
    /// independently of the wall clock.
    pub sequence: u64,
    /// UTC, `YYYY-MM-DDTHH:MM:SSZ`.
    pub created_at: String,
    pub created_unix: i64,
    /// Previous set this one is incremental to; `None` for a full set.
    pub parent: Option<String>,
    pub entries: Vec<ManifestEntry>,
    /// Fields written by a newer Fotobase: kept verbatim.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Progress after each processed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupProgress {
    pub done: usize,
    pub total: usize,
    pub current: String,
    pub bytes_copied: u64,
}

/// Outcome of a committed set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupReport {
    pub set_id: String,
    pub set_path: String,
    pub parent: Option<String>,
    pub files_total: usize,
    /// Files whose blob was written into this set.
    pub files_copied: usize,
    /// Files referencing a blob already stored in an earlier set.
    pub files_reused: usize,
    pub bytes_copied: u64,
}

/// One set as listed in the Backup Manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetSummary {
    pub id: String,
    pub created_at: String,
    pub created_unix: i64,
    pub parent: Option<String>,
    pub files: usize,
    /// Files whose blob lives in this set (the set's own delta).
    pub new_files: usize,
    pub new_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    /// Checked before anything is written: missing, unplugged or read-only.
    #[error("backup destination {dest} is not available or not writable: {source}")]
    Unavailable { dest: PathBuf, source: std::io::Error },
    /// Writing failed mid-run (destination full or disconnected).
    #[error(
        "backup stopped writing {path}: {source} — destination full or disconnected? \
         No backup set was recorded."
    )]
    Destination { path: PathBuf, source: std::io::Error },
    #[error("cannot read {path} for backup: {source}")]
    Source { path: PathBuf, source: std::io::Error },
    #[error("backup cancelled; no backup set was recorded")]
    Cancelled,
    #[error(transparent)]
    Catalog(#[from] CatalogError),
}

/// Caller hooks: progress, cooperative cancel, and pacing (the app sleeps in
/// `pace` while on battery — hashing is throttled, PLAN §10).
pub struct BackupHooks<'a> {
    pub progress: &'a dyn Fn(&BackupProgress),
    pub cancelled: &'a dyn Fn() -> bool,
    /// Called before each file is read and hashed.
    pub pace: &'a dyn Fn(),
}

fn no_progress(_: &BackupProgress) {}
fn never_cancelled() -> bool {
    false
}
fn no_pace() {}

impl BackupHooks<'static> {
    /// No progress, never cancelled, never paced.
    pub fn none() -> Self {
        BackupHooks { progress: &no_progress, cancelled: &never_cancelled, pace: &no_pace }
    }
}

/// How often scheduled backups run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackupSchedule {
    #[default]
    Off,
    Daily,
    Weekly,
}

impl BackupSchedule {
    fn interval_secs(self) -> Option<i64> {
        match self {
            BackupSchedule::Off => None,
            BackupSchedule::Daily => Some(86_400),
            BackupSchedule::Weekly => Some(7 * 86_400),
        }
    }

    /// Is a scheduled backup due, given the newest set's creation time?
    pub fn is_due(self, last_unix: Option<i64>, now_unix: i64) -> bool {
        match (self.interval_secs(), last_unix) {
            (None, _) => false,
            (Some(_), None) => true,
            (Some(interval), Some(last)) => now_unix - last >= interval,
        }
    }
}

/// The library's backup sources the catalog knows about: every original's
/// sidecar and every folder's `.fotobase` flag file that exist on disk, plus
/// `albums.json` beside the catalog. App settings files are the caller's.
pub fn library_sources(cat: &Catalog) -> crate::Result<Vec<BackupSource>> {
    let mut out = Vec::new();
    let mut push = |kind: EntryKind, path: PathBuf| {
        if path.is_file() {
            out.push(BackupSource { kind, path });
        }
    };
    let albums = cat.albums_json_path();
    push(EntryKind::Settings, albums);

    let mut folders = cat.conn().prepare("SELECT path FROM folders ORDER BY path")?;
    for dir in folders.query_map([], |r| r.get::<_, String>(0))? {
        push(EntryKind::Sidecar, fotobase_metadata::FolderMeta::path(Path::new(&dir?)));
    }
    let mut files = cat.conn().prepare("SELECT path FROM files ORDER BY path")?;
    for original in files.query_map([], |r| r.get::<_, String>(0))? {
        push(EntryKind::Sidecar, sidecar_path(&original?));
    }
    Ok(out)
}

/// Read `<set_dir>/manifest.json`.
pub fn read_manifest(set_dir: &Path) -> std::io::Result<Manifest> {
    let bytes = std::fs::read(set_dir.join(MANIFEST_FILE))?;
    let m: Manifest = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
    if m.format != MANIFEST_FORMAT {
        return Err(std::io::Error::other(format!("not a backup manifest: {}", m.format)));
    }
    Ok(m)
}

/// Every committed set under `dest`, oldest first. A destination without a
/// backup folder has no sets.
pub fn list_sets(dest: &Path) -> std::io::Result<Vec<SetSummary>> {
    Ok(manifests(&dest.join(BACKUP_DIR))?
        .into_iter()
        .map(|m| {
            let own = m.entries.iter().filter(|e| e.set == m.id);
            SetSummary {
                files: m.entries.len(),
                new_files: own.clone().count(),
                new_bytes: own.map(|e| e.size).sum(),
                id: m.id,
                created_at: m.created_at,
                created_unix: m.created_unix,
                parent: m.parent,
            }
        })
        .collect())
}

/// Creation time of the newest set under `dest` (scheduling input).
pub fn last_backup_unix(dest: &Path) -> Option<i64> {
    manifests(&dest.join(BACKUP_DIR)).ok()?.last().map(|m| m.created_unix)
}

fn manifests(root: &Path) -> std::io::Result<Vec<Manifest>> {
    let rd = match std::fs::read_dir(root) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out: Vec<Manifest> = rd
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().ends_with(PARTIAL_SUFFIX))
        .filter(|e| e.path().is_dir())
        .filter_map(|e| read_manifest(&e.path()).ok())
        .collect();
    out.sort_by(|a, b| a.sequence.cmp(&b.sequence).then_with(|| a.id.cmp(&b.id)));
    Ok(out)
}

/// Write one backup set of `catalog` (snapshot) plus `sources` into
/// `dest/fotobase-backup/`, incremental to the newest set already there.
pub fn run_backup(
    dest: &Path,
    catalog: Option<&Catalog>,
    sources: &[BackupSource],
    hooks: &BackupHooks<'_>,
) -> Result<BackupReport, BackupError> {
    let root = prepare_root(dest)?;
    let previous = manifests(&root)
        .map_err(|source| BackupError::Unavailable { dest: dest.to_path_buf(), source })?
        .pop();

    let now = SystemTime::now();
    let created_unix =
        now.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    let id = unique_set_id(&root, created_unix);
    let staging = root.join(format!("{id}{PARTIAL_SUFFIX}"));
    let final_dir = root.join(&id);

    let mut run = Run {
        id: id.clone(),
        files: staging.join(FILES_DIR),
        hooks,
        previous: previous
            .as_ref()
            .map(|m| m.entries.iter().map(|e| ((e.kind, e.path.clone()), e.clone())).collect())
            .unwrap_or_default(),
        stored: previous
            .as_ref()
            .map(|m| m.entries.iter().map(|e| (e.sha256.clone(), e.set.clone())).collect())
            .unwrap_or_default(),
        entries: Vec::new(),
        copied: 0,
        reused: 0,
        bytes: 0,
        total: sources.len() + usize::from(catalog.is_some()),
    };

    let result = std::fs::create_dir_all(&run.files)
        .map_err(|source| BackupError::Destination { path: run.files.clone(), source })
        .and_then(|()| run.stage(catalog, sources))
        .and_then(|()| {
            if (hooks.cancelled)() {
                return Err(BackupError::Cancelled);
            }
            std::fs::rename(&staging, &final_dir)
                .map_err(|source| BackupError::Destination { path: final_dir.clone(), source })
        })
        .and_then(|()| {
            let manifest = Manifest {
                format: MANIFEST_FORMAT.into(),
                version: MANIFEST_VERSION,
                id: id.clone(),
                sequence: previous.as_ref().map_or(1, |m| m.sequence + 1),
                created_at: utc_stamp(created_unix),
                created_unix,
                parent: previous.as_ref().map(|m| m.id.clone()),
                entries: std::mem::take(&mut run.entries),
                extra: Map::new(),
            };
            write_manifest(&final_dir, &manifest)
        });

    if let Err(err) = result {
        // Best-effort: whatever is left has no manifest, so it is never
        // mistaken for a set; a later run sweeps leftover `.partial` dirs.
        let _ = std::fs::remove_dir_all(&staging);
        let _ = std::fs::remove_dir_all(&final_dir);
        return Err(err);
    }
    Ok(BackupReport {
        set_id: id,
        set_path: final_dir.to_string_lossy().into_owned(),
        parent: previous.map(|m| m.id),
        files_total: run.copied + run.reused,
        files_copied: run.copied,
        files_reused: run.reused,
        bytes_copied: run.bytes,
    })
}

/// Verify the destination before writing anything: it must already exist
/// (an unplugged drive's mount point must not be recreated on the system
/// disk) and accept a probe file. Sweeps leftover `.partial` runs.
fn prepare_root(dest: &Path) -> Result<PathBuf, BackupError> {
    let unavailable = |source| BackupError::Unavailable { dest: dest.to_path_buf(), source };
    if !dest.is_dir() {
        return Err(unavailable(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not an existing folder",
        )));
    }
    let root = dest.join(BACKUP_DIR);
    std::fs::create_dir_all(&root).map_err(unavailable)?;
    let probe = root.join(PROBE_FILE);
    std::fs::write(&probe, b"fotobase").map_err(unavailable)?;
    std::fs::remove_file(&probe).map_err(unavailable)?;

    for e in std::fs::read_dir(&root).map_err(unavailable)?.flatten() {
        if e.file_name().to_string_lossy().ends_with(PARTIAL_SUFFIX) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    Ok(root)
}

struct Run<'h> {
    id: String,
    files: PathBuf,
    hooks: &'h BackupHooks<'h>,
    /// Previous set's entries by (kind, path).
    previous: HashMap<(EntryKind, String), ManifestEntry>,
    /// Blob hash → set holding it (previous chain + this run).
    stored: HashMap<String, String>,
    entries: Vec<ManifestEntry>,
    copied: usize,
    reused: usize,
    bytes: u64,
    total: usize,
}

impl Run<'_> {
    fn stage(
        &mut self,
        catalog: Option<&Catalog>,
        sources: &[BackupSource],
    ) -> Result<(), BackupError> {
        if let Some(cat) = catalog {
            self.check_cancel()?;
            self.stage_catalog(cat)?;
            self.report(cat.path());
        }
        for src in sources {
            self.check_cancel()?;
            self.stage_file(src)?;
            self.report(&src.path);
        }
        Ok(())
    }

    fn check_cancel(&self) -> Result<(), BackupError> {
        if (self.hooks.cancelled)() {
            Err(BackupError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn report(&self, current: &Path) {
        (self.hooks.progress)(&BackupProgress {
            done: self.entries.len(),
            total: self.total,
            current: current.to_string_lossy().into_owned(),
            bytes_copied: self.bytes,
        });
    }

    /// `VACUUM INTO` a consistent snapshot (WAL included) straight into the
    /// staging area, then hash it. Always re-snapshotted: the database's
    /// own mtime says nothing about uncheckpointed WAL content.
    fn stage_catalog(&mut self, cat: &Catalog) -> Result<(), BackupError> {
        (self.hooks.pace)();
        let tmp = self.files.join(TMP_BLOB);
        let sql = format!("VACUUM INTO '{}'", tmp.to_string_lossy().replace('\'', "''"));
        cat.conn().execute_batch(&sql).map_err(|e| BackupError::Destination {
            path: tmp.clone(),
            source: std::io::Error::other(e),
        })?;
        let (sha256, size) = hash_file(&tmp)
            .map_err(|source| BackupError::Destination { path: tmp.clone(), source })?;
        let (secs, nanos) = (now_secs(), 0);
        self.commit_blob(EntryKind::Catalog, cat.path(), sha256, size, secs, nanos)
    }

    fn stage_file(&mut self, src: &BackupSource) -> Result<(), BackupError> {
        let meta = match std::fs::metadata(&src.path) {
            Ok(m) => m,
            // Deleted between listing and backup: nothing to keep.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!(path = %src.path.display(), "backup source vanished; skipped");
                return Ok(());
            }
            Err(source) => return Err(BackupError::Source { path: src.path.clone(), source }),
        };
        let (secs, nanos) = mtime(&meta);
        let key = (src.kind, src.path.to_string_lossy().into_owned());

        // PLAN §5 incremental skip: same size + mtime → same content.
        if let Some(prev) = self.previous.get(&key) {
            if prev.size == meta.len() && prev.mtime_secs == secs && prev.mtime_nanos == nanos {
                self.entries.push(prev.clone());
                self.reused += 1;
                return Ok(());
            }
        }

        (self.hooks.pace)();
        let tmp = self.files.join(TMP_BLOB);
        let (sha256, size) = copy_hashed(&src.path, &tmp)?;
        self.commit_blob(src.kind, &src.path, sha256, size, secs, nanos)
    }

    /// Move the staged `files/.incoming` into place under its hash, or drop
    /// it when an earlier set (or this run) already stores that content.
    fn commit_blob(
        &mut self,
        kind: EntryKind,
        path: &Path,
        sha256: String,
        size: u64,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> Result<(), BackupError> {
        let tmp = self.files.join(TMP_BLOB);
        let set = match self.stored.get(&sha256) {
            Some(set) => {
                std::fs::remove_file(&tmp)
                    .map_err(|source| BackupError::Destination { path: tmp.clone(), source })?;
                self.reused += 1;
                set.clone()
            }
            None => {
                let blob = self.files.join(&sha256);
                std::fs::rename(&tmp, &blob)
                    .map_err(|source| BackupError::Destination { path: blob, source })?;
                self.stored.insert(sha256.clone(), self.id.clone());
                self.copied += 1;
                self.bytes += size;
                self.id.clone()
            }
        };
        self.entries.push(ManifestEntry {
            kind,
            path: path.to_string_lossy().into_owned(),
            size,
            mtime_secs,
            mtime_nanos,
            sha256,
            set,
        });
        Ok(())
    }
}

/// Stream `src` into `dst` while hashing; read failures are the source's,
/// write failures the destination's. The blob is synced before returning.
fn copy_hashed(src: &Path, dst: &Path) -> Result<(String, u64), BackupError> {
    let source_err = |source| BackupError::Source { path: src.to_path_buf(), source };
    let dest_err = |source| BackupError::Destination { path: dst.to_path_buf(), source };
    let mut input = std::fs::File::open(src).map_err(source_err)?;
    let mut output = std::fs::File::create(dst).map_err(dest_err)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = input.read(&mut buf).map_err(source_err)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        output.write_all(&buf[..n]).map_err(dest_err)?;
        size += n as u64;
    }
    output.sync_all().map_err(dest_err)?;
    Ok((hex(&hasher.finalize()), size))
}

pub(crate) fn hash_file(path: &Path) -> std::io::Result<(String, u64)> {
    let mut input = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((hex(&hasher.finalize()), size))
}

fn write_manifest(set_dir: &Path, manifest: &Manifest) -> Result<(), BackupError> {
    let tmp = set_dir.join(format!("{MANIFEST_FILE}.tmp"));
    let dest_err = |source| BackupError::Destination { path: tmp.clone(), source };
    let json = serde_json::to_vec_pretty(manifest).expect("manifest serializes infallibly");
    let mut f = std::fs::File::create(&tmp).map_err(dest_err)?;
    f.write_all(&json).map_err(dest_err)?;
    f.sync_all().map_err(dest_err)?;
    drop(f);
    let target = set_dir.join(MANIFEST_FILE);
    std::fs::rename(&tmp, &target)
        .map_err(|source| BackupError::Destination { path: target, source })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn mtime(meta: &std::fs::Metadata) -> (i64, u32) {
    match meta.modified().map(|t| t.duration_since(SystemTime::UNIX_EPOCH)) {
        Ok(Ok(d)) => (d.as_secs() as i64, d.subsec_nanos()),
        _ => (0, 0),
    }
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

/// `YYYYMMDDTHHMMSSZ`, suffixed `-N` when a set of that second exists.
fn unique_set_id(root: &Path, unix: i64) -> String {
    let (y, m, d) = civil_from_days(unix.div_euclid(86_400));
    let tod = unix.rem_euclid(86_400);
    let base =
        format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", tod / 3600, (tod % 3600) / 60, tod % 60);
    let taken =
        |id: &str| root.join(id).exists() || root.join(format!("{id}{PARTIAL_SUFFIX}")).exists();
    let mut id = base.clone();
    let mut n = 1u32;
    while taken(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

fn utc_stamp(unix: i64) -> String {
    let (y, m, d) = civil_from_days(unix.div_euclid(86_400));
    let tod = unix.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", tod / 3600, (tod % 3600) / 60, tod % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_due_math() {
        let day = 86_400;
        assert!(!BackupSchedule::Off.is_due(None, 0));
        assert!(BackupSchedule::Daily.is_due(None, 0), "never backed up → due");
        assert!(!BackupSchedule::Daily.is_due(Some(10 * day), 11 * day - 1));
        assert!(BackupSchedule::Daily.is_due(Some(10 * day), 11 * day));
        assert!(!BackupSchedule::Weekly.is_due(Some(10 * day), 16 * day));
        assert!(BackupSchedule::Weekly.is_due(Some(10 * day), 17 * day));
    }

    #[test]
    fn set_ids_sort_and_stamp_in_utc() {
        assert_eq!(utc_stamp(1_795_000_000), "2026-11-18T11:06:40Z");
        let dir = tempfile::tempdir().unwrap();
        let a = unique_set_id(dir.path(), 1_795_000_000);
        assert_eq!(a, "20261118T110640Z");
        std::fs::create_dir(dir.path().join(&a)).unwrap();
        assert_eq!(unique_set_id(dir.path(), 1_795_000_000), "20261118T110640Z-1");
    }
}
