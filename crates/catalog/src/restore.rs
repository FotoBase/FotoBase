//! Restore from backup sets (TASK-08-015, PLAN §6 P4 "Incremental backup +
//! restore", §9 Data safety): the reverse of [`crate::backup`].
//!
//! Restore replaces the catalog, the XMP sidecars and the settings files —
//! never originals — and reproduces them byte-identical to the set.
//!
//! # Order of operations
//!
//! 1. **Validate** the whole set read-only: every manifest entry's blob
//!    (resolved through the chain of earlier sets it references) must exist,
//!    match its recorded size and hash to its recorded SHA-256. Nothing is
//!    written until the set is proven good; a corrupt set is rejected with
//!    the affected files named.
//! 2. **Plan**: resolve each entry's target, skip files already identical,
//!    refuse to overwrite differing files without explicit confirmation.
//! 3. **Stage**: copy every blob next to its target as
//!    `<name>.fb-restore.tmp`, re-hashing while copying. A failure here
//!    removes the staged files; the library was not touched.
//! 4. **Swap**: a journal listing every operation is written first, then
//!    each target is moved aside to `<name>.fb-restore.bak` and its staged
//!    file renamed into place. Any failure rolls every swap back from the
//!    `.bak` files. If the process dies mid-swap, [`recover_interrupted`]
//!    performs the same rollback (or, if the swap had finished, the
//!    cleanup) on the next start.
//!
//! Files cannot all be swapped by one rename (sidecars live next to the
//! originals, all over the disk), so the journal plus per-file `.bak` is the
//! atomicity: a half-restored library is never the *resting* state.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::albums::ALBUMS_FILE;
use crate::backup::{
    hash_file, read_manifest, EntryKind, Manifest, ManifestEntry, BACKUP_DIR, FILES_DIR,
};

/// Journal of a swap in progress, in the catalog's directory.
pub const JOURNAL_FILE: &str = ".fb-restore-journal.json";
const TMP_SUFFIX: &str = ".fb-restore.tmp";
const BAK_SUFFIX: &str = ".fb-restore.bak";

/// Where and how to restore.
#[derive(Debug, Clone)]
pub struct RestoreOptions {
    /// Where the catalog database goes (the active catalog's location,
    /// whatever path the set recorded). Its directory holds the journal.
    pub catalog_path: PathBuf,
    /// The user explicitly agreed to replace files that differ.
    pub confirm_overwrite: bool,
}

/// Progress after each staged file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreProgress {
    pub done: usize,
    pub total: usize,
    pub current: String,
}

/// Failure injected by a test through [`RestoreHooks::fault`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    None,
    /// The swap step fails: exercises the rollback path.
    Fail,
    /// The process "dies": the swap stops with no rollback, leaving the
    /// journal for [`recover_interrupted`].
    Crash,
}

/// Caller hooks: progress, cooperative cancel (honored until the swap
/// starts), and a fault-injection seam called after the n-th swap.
pub struct RestoreHooks<'a> {
    pub progress: &'a dyn Fn(&RestoreProgress),
    pub cancelled: &'a dyn Fn() -> bool,
    pub fault: &'a dyn Fn(usize) -> Fault,
}

fn no_progress(_: &RestoreProgress) {}
fn never_cancelled() -> bool {
    false
}
fn no_fault(_: usize) -> Fault {
    Fault::None
}

impl RestoreHooks<'static> {
    /// No progress, never cancelled, no injected faults.
    pub fn none() -> Self {
        RestoreHooks { progress: &no_progress, cancelled: &never_cancelled, fault: &no_fault }
    }
}

/// One file of a set that cannot be restored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetProblem {
    /// Where the file lives in the library.
    pub path: String,
    pub reason: String,
}

/// Outcome of a restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreReport {
    pub set_id: String,
    pub files_restored: usize,
    /// Files already byte-identical to the set; left untouched.
    pub files_unchanged: usize,
    /// Sidecars whose folder no longer exists (not recreated: it may be an
    /// unplugged drive), by path.
    pub skipped: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error("backup set {0} not found")]
    SetNotFound(String),
    #[error("backup set {id} cannot be read: {source}")]
    Unreadable { id: String, source: std::io::Error },
    #[error(
        "backup set {id} is corrupt ({} problem(s), e.g. {}: {}); nothing was restored",
        .problems.len(), .problems[0].path, .problems[0].reason
    )]
    Corrupt { id: String, problems: Vec<SetProblem> },
    #[error("restoring replaces {count} existing file(s) that differ; confirmation required")]
    OverwriteNotConfirmed { count: usize },
    #[error("restore cancelled; nothing was changed")]
    Cancelled,
    #[error("restore failed at {path}: {source}; the previous library was kept")]
    Io { path: PathBuf, source: std::io::Error },
    /// Only produced by [`Fault::Crash`].
    #[error("restore interrupted")]
    Interrupted,
}

fn io_at(path: &Path) -> impl Fn(std::io::Error) -> RestoreError + '_ {
    move |source| RestoreError::Io { path: path.to_path_buf(), source }
}

fn set_dir(dest: &Path, set_id: &str) -> PathBuf {
    dest.join(BACKUP_DIR).join(set_id)
}

/// A bare directory or hash name: never a path that could leave the backup
/// folder.
fn plain_name(s: &str) -> bool {
    !s.is_empty() && !s.contains(['/', '\\']) && s != "." && s != ".." && !s.contains('\0')
}

fn read_set(dest: &Path, set_id: &str) -> Result<Manifest, RestoreError> {
    if !plain_name(set_id) {
        return Err(RestoreError::SetNotFound(set_id.to_string()));
    }
    match read_manifest(&set_dir(dest, set_id)) {
        Ok(m) => Ok(m),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(RestoreError::SetNotFound(set_id.to_string()))
        }
        Err(source) => Err(RestoreError::Unreadable { id: set_id.to_string(), source }),
    }
}

fn blob_path(dest: &Path, e: &ManifestEntry) -> Option<PathBuf> {
    let hash_ok = e.sha256.len() == 64 && e.sha256.bytes().all(|b| b.is_ascii_hexdigit());
    (plain_name(&e.set) && hash_ok)
        .then(|| dest.join(BACKUP_DIR).join(&e.set).join(FILES_DIR).join(&e.sha256))
}

/// Prove every file of the set is present and intact, writing nothing.
/// Returns the manifest on success.
pub fn validate_set(dest: &Path, set_id: &str) -> Result<Manifest, RestoreError> {
    validate(dest, set_id, &|| false)
}

fn validate(
    dest: &Path,
    set_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Manifest, RestoreError> {
    let manifest = read_set(dest, set_id)?;
    let mut verdicts: std::collections::HashMap<(String, String), Option<String>> =
        std::collections::HashMap::new();
    let mut problems = Vec::new();
    for e in &manifest.entries {
        if cancelled() {
            return Err(RestoreError::Cancelled);
        }
        let verdict = verdicts
            .entry((e.set.clone(), e.sha256.clone()))
            .or_insert_with(|| check_blob(dest, e));
        if let Some(reason) = verdict {
            problems.push(SetProblem { path: e.path.clone(), reason: reason.clone() });
        }
    }
    if problems.is_empty() {
        Ok(manifest)
    } else {
        Err(RestoreError::Corrupt { id: set_id.to_string(), problems })
    }
}

/// Why a blob is unusable, or `None` when it matches its manifest entry.
fn check_blob(dest: &Path, e: &ManifestEntry) -> Option<String> {
    let Some(blob) = blob_path(dest, e) else {
        return Some("manifest entry names an invalid blob".into());
    };
    match hash_file(&blob) {
        Ok((sha, size)) if sha == e.sha256 && size == e.size => None,
        Ok((_, size)) if size != e.size => {
            Some(format!("size mismatch (expected {} bytes, found {size})", e.size))
        }
        Ok(_) => Some("content hash mismatch".into()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Some(format!("blob missing from set {}", e.set))
        }
        Err(err) => Some(format!("blob unreadable: {err}")),
    }
}

/// Where a manifest entry goes on this machine, or why it must not.
fn target_for(
    e: &ManifestEntry,
    recorded_catalog_dir: Option<&Path>,
    opts: &RestoreOptions,
) -> Result<PathBuf, String> {
    if e.kind == EntryKind::Catalog {
        return Ok(opts.catalog_path.clone());
    }
    let path = PathBuf::from(&e.path);
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("target path is not an absolute, normalized path".into());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let allowed = match e.kind {
        EntryKind::Sidecar => name.ends_with(".xmp") || name == fotobase_metadata::FOLDER_META_FILE,
        EntryKind::Settings => name.ends_with(".json"),
        EntryKind::Catalog => true,
    };
    if !allowed {
        return Err("not a file Fotobase restores".into());
    }
    // `albums.json` lives beside the catalog: it follows the catalog when
    // the catalog is restored somewhere else.
    if name == ALBUMS_FILE && path.parent() == recorded_catalog_dir {
        if let Some(dir) = opts.catalog_path.parent() {
            return Ok(dir.join(ALBUMS_FILE));
        }
    }
    Ok(path)
}

struct Op {
    target: PathBuf,
    /// Blob to stage; `None` for a file that is only moved aside (the stale
    /// `-wal`/`-shm` of a replaced catalog).
    blob: Option<PathBuf>,
    tmp: PathBuf,
    bak: PathBuf,
    had_target: bool,
    must_create_parent: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct JournalOp {
    target: PathBuf,
    /// Present when a staged file was to be swapped in.
    tmp: Option<PathBuf>,
    bak: PathBuf,
    had_target: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Journal {
    /// True once every swap finished: only cleanup is left.
    committed: bool,
    ops: Vec<JournalOp>,
}

fn side_path(target: &Path, suffix: &str) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    target.with_file_name(name)
}

fn journal_path(catalog_path: &Path) -> PathBuf {
    catalog_path.parent().unwrap_or(Path::new("")).join(JOURNAL_FILE)
}

fn write_journal(path: &Path, journal: &Journal) -> std::io::Result<()> {
    let tmp = side_path(path, ".tmp");
    let json = serde_json::to_vec_pretty(journal).expect("journal serializes infallibly");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(&json)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

/// Restore `set_id` found under `dest`. See the module docs for the order of
/// operations and the guarantees.
pub fn restore_set(
    dest: &Path,
    set_id: &str,
    opts: &RestoreOptions,
    hooks: &RestoreHooks<'_>,
) -> Result<RestoreReport, RestoreError> {
    let journal_file = journal_path(&opts.catalog_path);
    // A previous run that died mid-swap must be undone before a new one
    // starts, or its `.bak` files would be overwritten.
    if let Some(dir) = journal_file.parent().filter(|d| !d.as_os_str().is_empty()) {
        recover_interrupted(dir).map_err(io_at(&journal_file))?;
    }

    let manifest = validate(dest, set_id, hooks.cancelled)?;
    let (ops, unchanged, skipped) = plan(dest, &manifest, opts)?;
    let report = |restored: usize| RestoreReport {
        set_id: set_id.to_string(),
        files_restored: restored,
        files_unchanged: unchanged,
        skipped: skipped.clone(),
    };
    let total = ops.iter().filter(|o| o.blob.is_some()).count();
    if ops.is_empty() {
        return Ok(report(0));
    }

    stage(&ops, total, hooks)?;

    let mut journal = Journal {
        committed: false,
        ops: ops
            .iter()
            .map(|o| JournalOp {
                target: o.target.clone(),
                tmp: o.blob.is_some().then(|| o.tmp.clone()),
                bak: o.bak.clone(),
                had_target: o.had_target,
            })
            .collect(),
    };
    if let Err(e) = write_journal(&journal_file, &journal) {
        discard_staged(&ops);
        return Err(io_at(&journal_file)(e));
    }

    if let Err(err) = swap(&ops, hooks) {
        if !matches!(err, RestoreError::Interrupted) {
            // A failed rollback keeps the journal: the next start retries.
            if rollback(&journal.ops).is_ok() {
                let _ = std::fs::remove_file(&journal_file);
            }
        }
        return Err(err);
    }

    journal.committed = true;
    write_journal(&journal_file, &journal).map_err(io_at(&journal_file))?;
    cleanup(&journal.ops);
    let _ = std::fs::remove_file(&journal_file);
    Ok(report(total))
}

type Plan = (Vec<Op>, usize, Vec<String>);

fn plan(dest: &Path, manifest: &Manifest, opts: &RestoreOptions) -> Result<Plan, RestoreError> {
    let recorded_catalog_dir = manifest
        .entries
        .iter()
        .find(|e| e.kind == EntryKind::Catalog)
        .and_then(|e| Path::new(&e.path).parent().map(Path::to_path_buf));

    let mut ops = Vec::new();
    let mut unchanged = 0;
    let mut skipped = Vec::new();
    let mut differing = 0;
    let mut problems = Vec::new();
    let mut seen = HashSet::new();
    let mut companions = Vec::new();

    for e in &manifest.entries {
        let target = match target_for(e, recorded_catalog_dir.as_deref(), opts) {
            Ok(t) => t,
            Err(reason) => {
                problems.push(SetProblem { path: e.path.clone(), reason });
                continue;
            }
        };
        if !seen.insert(target.clone()) {
            problems.push(SetProblem {
                path: e.path.clone(),
                reason: "two entries restore to the same file".into(),
            });
            continue;
        }
        let parent_exists = target.parent().is_some_and(Path::is_dir);
        if e.kind == EntryKind::Sidecar && !parent_exists {
            skipped.push(e.path.clone());
            continue;
        }
        let had_target = match std::fs::symlink_metadata(&target) {
            Ok(m) if m.is_file() => true,
            Ok(_) => {
                return Err(io_at(&target)(std::io::Error::other("not a regular file")));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
            Err(err) => return Err(io_at(&target)(err)),
        };
        if had_target {
            match hash_file(&target) {
                Ok((sha, _)) if sha == e.sha256 => {
                    unchanged += 1;
                    continue;
                }
                _ => differing += 1,
            }
        }
        if e.kind == EntryKind::Catalog {
            // A stale write-ahead log would be replayed over the restored
            // database: it goes aside with the old one.
            for ext in ["-wal", "-shm"] {
                let mut name = target.file_name().unwrap_or_default().to_os_string();
                name.push(ext);
                let side = target.with_file_name(name);
                if side.is_file() {
                    companions.push(Op {
                        bak: side_path(&side, BAK_SUFFIX),
                        tmp: side_path(&side, TMP_SUFFIX),
                        target: side,
                        blob: None,
                        had_target: true,
                        must_create_parent: false,
                    });
                }
            }
        }
        ops.push(Op {
            blob: blob_path(dest, e),
            tmp: side_path(&target, TMP_SUFFIX),
            bak: side_path(&target, BAK_SUFFIX),
            had_target,
            must_create_parent: e.kind != EntryKind::Sidecar,
            target,
        });
    }

    if !problems.is_empty() {
        return Err(RestoreError::Corrupt { id: manifest.id.clone(), problems });
    }
    if differing > 0 && !opts.confirm_overwrite {
        return Err(RestoreError::OverwriteNotConfirmed { count: differing });
    }
    ops.extend(companions);
    Ok((ops, unchanged, skipped))
}

fn discard_staged(ops: &[Op]) {
    for op in ops.iter().filter(|o| o.blob.is_some()) {
        let _ = std::fs::remove_file(&op.tmp);
    }
}

/// Copy every blob next to its target, verifying the hash again on the way.
fn stage(ops: &[Op], total: usize, hooks: &RestoreHooks<'_>) -> Result<(), RestoreError> {
    let result = (|| {
        for (done, op) in ops.iter().filter(|o| o.blob.is_some()).enumerate() {
            if (hooks.cancelled)() {
                return Err(RestoreError::Cancelled);
            }
            if op.must_create_parent {
                if let Some(parent) = op.target.parent() {
                    std::fs::create_dir_all(parent).map_err(io_at(parent))?;
                }
            }
            copy_verified(op.blob.as_deref().expect("filtered"), &op.tmp)?;
            (hooks.progress)(&RestoreProgress {
                done: done + 1,
                total,
                current: op.target.to_string_lossy().into_owned(),
            });
        }
        Ok(())
    })();
    if result.is_err() {
        discard_staged(ops);
    }
    result
}

fn copy_verified(blob: &Path, tmp: &Path) -> Result<(), RestoreError> {
    let expected = blob.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut input = std::fs::File::open(blob).map_err(io_at(blob))?;
    let mut output = std::fs::File::create(tmp).map_err(io_at(tmp))?;
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = input.read(&mut buf).map_err(io_at(blob))?;
        if n == 0 {
            break;
        }
        sha2::Digest::update(&mut hasher, &buf[..n]);
        output.write_all(&buf[..n]).map_err(io_at(tmp))?;
    }
    output.sync_all().map_err(io_at(tmp))?;
    let actual: String =
        sha2::Digest::finalize(hasher).iter().map(|b| format!("{b:02x}")).collect();
    if actual != expected {
        return Err(io_at(blob)(std::io::Error::other("blob changed while restoring")));
    }
    Ok(())
}

/// Move each target aside and its staged file into place.
fn swap(ops: &[Op], hooks: &RestoreHooks<'_>) -> Result<(), RestoreError> {
    for (i, op) in ops.iter().enumerate() {
        if op.had_target {
            std::fs::rename(&op.target, &op.bak).map_err(io_at(&op.target))?;
        }
        if op.blob.is_some() {
            std::fs::rename(&op.tmp, &op.target).map_err(io_at(&op.target))?;
        }
        match (hooks.fault)(i + 1) {
            Fault::None => {}
            Fault::Fail => {
                return Err(io_at(&op.target)(std::io::Error::other("injected failure")));
            }
            Fault::Crash => return Err(RestoreError::Interrupted),
        }
    }
    Ok(())
}

/// Undo a swap that did not finish: put every `.bak` back and drop whatever
/// was staged or already swapped in.
fn rollback(ops: &[JournalOp]) -> std::io::Result<()> {
    for op in ops.iter().rev() {
        let staged_left = op.tmp.as_ref().is_some_and(|t| t.exists());
        if op.bak.exists() {
            match std::fs::remove_file(&op.target) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
            std::fs::rename(&op.bak, &op.target)?;
        } else if !op.had_target && op.tmp.is_some() && !staged_left {
            // New file swapped in before the stop: remove it.
            match std::fs::remove_file(&op.target) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        if let Some(tmp) = op.tmp.as_ref().filter(|_| staged_left) {
            std::fs::remove_file(tmp)?;
        }
    }
    Ok(())
}

/// Drop the `.bak` copies once the new files are the library.
fn cleanup(ops: &[JournalOp]) {
    for op in ops {
        let _ = std::fs::remove_file(&op.bak);
        if let Some(tmp) = &op.tmp {
            let _ = std::fs::remove_file(tmp);
        }
    }
}

/// Finish what a killed restore left behind: roll an unfinished swap back
/// to the previous library, or clean up after one that had completed.
/// `dir` is the catalog's directory. Returns whether a journal was found.
/// Call before opening the catalog at startup; safe to call repeatedly.
pub fn recover_interrupted(dir: &Path) -> std::io::Result<bool> {
    let journal_file = dir.join(JOURNAL_FILE);
    let bytes = match std::fs::read(&journal_file) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let journal: Journal = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
    if journal.committed {
        cleanup(&journal.ops);
    } else {
        rollback(&journal.ops)?;
    }
    std::fs::remove_file(&journal_file)?;
    Ok(true)
}
