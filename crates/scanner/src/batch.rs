//! Batch rename and batch move (TASK-03-011, PLAN §2 B "Batch rename / move",
//! 00-010 §4.5).
//!
//! Planning is pure-ish and side-effect free so the UI can preview: a
//! [`RenameSpec`] pattern (`{stem}`, `{n}`, `{date}`) or a destination folder
//! turns a selection into a [`BatchPlan`] of [`PlannedMove`]s plus the
//! [`Collision`]s that make a target unusable. [`CollisionPolicy`] decides
//! whether a clash is refused (reported, nothing applies) or auto-resolved by
//! suffixing ` (2)`, ` (3)`…
//!
//! [`apply_batch`] performs the real file moves. Per photo the order is:
//! original → sidecar (`IMG.jpg.xmp`, plus its `.fb-bak`) → catalog row. A
//! row keeps its id, so stars/tags/albums stay attached and the next scan
//! matches the file by (mtime, size) instead of seeing a new photo. Nothing
//! is ever overwritten, and the original's bytes are never rewritten — only
//! its location/name changes (a cross-device move copies, then deletes the
//! source only after the copy landed).
//!
//! Failure handling: the first failure aborts the rest of the batch and the
//! already-applied photos are rolled back in reverse, so the batch is
//! all-or-nothing and the error says what failed. A hard kill mid-batch
//! cannot roll back; it leaves at most one photo with its original at the
//! new path and the catalog/sidecar at the old one. The next scan treats
//! that as an external move: 03-003 re-association restores the row's user
//! data and `follow_sidecar` brings the orphaned sidecar along.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use fotobase_catalog::{iso_date, sidecar_path, BatchFile, Catalog};
use serde::{Deserialize, Serialize};

/// Longest file name the supported filesystems accept (bytes).
const MAX_NAME_BYTES: usize = 255;
/// Upper bound on ` (n)` suffix probing per file before giving up.
const MAX_SUFFIX_TRIES: u32 = 10_000;
/// Windows reserved device names; refused on every OS so a library stays
/// portable.
const RESERVED_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Rename pattern plus its counter settings.
///
/// Tokens: `{stem}` original file stem, `{n}` counter (`start + step * index`,
/// zero-padded to `pad` digits), `{date}` capture date `YYYY-MM-DD` (falls
/// back to the file's mtime). The original extension is always kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameSpec {
    pub pattern: String,
    pub start: u32,
    pub step: u32,
    pub pad: u8,
}

/// What to do when a target name is already taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollisionPolicy {
    /// Report the collision; the plan must not be applied.
    Refuse,
    /// Resolve it by suffixing ` (2)`, ` (3)`…
    AutoSuffix,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    #[error("invalid rename pattern: {0}")]
    Invalid(String),
}

/// One file's move, expressed as full paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedMove {
    pub id: i64,
    pub from: String,
    pub to: String,
}

impl PlannedMove {
    /// The move that undoes this one.
    pub fn inverse(&self) -> PlannedMove {
        PlannedMove { id: self.id, from: self.to.clone(), to: self.from.clone() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollisionReason {
    /// Another photo of the same batch gets the same target.
    DuplicateTarget,
    /// A file (possibly another source of this batch) already has that name.
    TargetExists,
    /// The target's `.xmp` sidecar already exists — it is not ours to replace.
    SidecarExists,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Collision {
    pub id: i64,
    pub from: String,
    pub to: String,
    pub reason: CollisionReason,
}

/// A previewable batch: moves that are safe to apply, and the collisions
/// that (under [`CollisionPolicy::Refuse`]) must be resolved first.
/// Photos whose name would not change are left out of both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct BatchPlan {
    pub moves: Vec<PlannedMove>,
    pub collisions: Vec<Collision>,
}

impl BatchPlan {
    pub fn is_applicable(&self) -> bool {
        self.collisions.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BatchError {
    /// The batch stopped at `failed` and was rolled back.
    #[error("batch aborted at {}: {reason}", failed.from)]
    Aborted {
        failed: PlannedMove,
        reason: String,
        /// Photos that had been moved and were put back.
        rolled_back: usize,
        /// Photos that could not be put back (needs the user's attention).
        rollback_failures: Vec<String>,
    },
}

// ---- pattern engine -----------------------------------------------------

enum Piece {
    Literal(String),
    Stem,
    Counter,
    Date,
}

fn parse_pattern(pattern: &str) -> Result<Vec<Piece>, PatternError> {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                let mut token = String::new();
                let mut closed = false;
                for t in chars.by_ref() {
                    if t == '}' {
                        closed = true;
                        break;
                    }
                    token.push(t);
                }
                if !closed {
                    return Err(PatternError::Invalid("unclosed `{`".into()));
                }
                if !literal.is_empty() {
                    pieces.push(Piece::Literal(std::mem::take(&mut literal)));
                }
                pieces.push(match token.as_str() {
                    "stem" => Piece::Stem,
                    "n" => Piece::Counter,
                    "date" => Piece::Date,
                    other => {
                        return Err(PatternError::Invalid(format!("unknown token `{{{other}}}`")))
                    }
                });
            }
            '}' => return Err(PatternError::Invalid("unmatched `}`".into())),
            c => literal.push(c),
        }
    }
    if !literal.is_empty() {
        pieces.push(Piece::Literal(literal));
    }
    Ok(pieces)
}

/// `YYYY-MM-DD` from an ISO or EXIF-style stamp (`2023-05-01T…`,
/// `2023:05:01 …`), `None` when `taken_at` is not date-shaped.
fn date_of(taken_at: &str) -> Option<String> {
    let head = taken_at.get(..10)?;
    let b = head.as_bytes();
    let shaped = b.iter().enumerate().all(|(i, c)| match i {
        4 | 7 => *c == b'-' || *c == b':',
        _ => c.is_ascii_digit(),
    });
    shaped.then(|| head.replace(':', "-"))
}

/// Render the new file stem (no extension) for the photo at `index` of the
/// selection. Errors when the pattern is malformed or the result is not a
/// legal file name on every supported OS.
pub fn render_name(
    spec: &RenameSpec,
    stem: &str,
    index: usize,
    taken_at: Option<&str>,
    mtime: f64,
) -> Result<String, PatternError> {
    let mut out = String::new();
    for piece in parse_pattern(&spec.pattern)? {
        match piece {
            Piece::Literal(s) => out.push_str(&s),
            Piece::Stem => out.push_str(stem),
            Piece::Counter => {
                let n = u64::from(spec.start) + u64::from(spec.step) * index as u64;
                out.push_str(&format!("{n:0>width$}", width = usize::from(spec.pad)));
            }
            Piece::Date => {
                out.push_str(&taken_at.and_then(date_of).unwrap_or_else(|| iso_date(mtime)));
            }
        }
    }
    validate_name(&out)?;
    Ok(out)
}

fn validate_name(name: &str) -> Result<(), PatternError> {
    let bad = |why: &str| Err(PatternError::Invalid(format!("{why}: {name:?}")));
    if name.trim().is_empty() {
        return bad("empty name");
    }
    if name == "." || name == ".." {
        return bad("reserved name");
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return bad("must not end with a dot or space");
    }
    if name.chars().any(|c| c.is_control() || "/\\:*?\"<>|".contains(c)) {
        return bad("contains a character that is not allowed in file names");
    }
    let first = name.split('.').next().unwrap_or(name).trim_end().to_lowercase();
    if RESERVED_NAMES.contains(&first.as_str()) {
        return bad("reserved device name");
    }
    if name.len() > MAX_NAME_BYTES {
        return bad("name too long");
    }
    Ok(())
}

// ---- planning -------------------------------------------------------------

/// Preview a rename of `items` (in selection order) to `spec`.
pub fn plan_rename(
    items: &[BatchFile],
    spec: &RenameSpec,
    policy: CollisionPolicy,
) -> Result<BatchPlan, PatternError> {
    parse_pattern(&spec.pattern)?;
    let mut targets = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let path = Path::new(&item.path);
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let name = render_name(spec, &stem, index, item.taken_at.as_deref(), item.mtime)?;
        let full = format!("{name}{ext}");
        if full.len() > MAX_NAME_BYTES {
            return Err(PatternError::Invalid(format!("name too long: {full:?}")));
        }
        targets.push(path.parent().unwrap_or(Path::new("")).join(full));
    }
    Ok(resolve(items, targets, policy))
}

/// Preview moving `items` into the folder `dest_dir`, names unchanged.
pub fn plan_move(items: &[BatchFile], dest_dir: &str, policy: CollisionPolicy) -> BatchPlan {
    let targets = items
        .iter()
        .map(|item| {
            let name = Path::new(&item.path).file_name().unwrap_or_default();
            Path::new(dest_dir).join(name)
        })
        .collect();
    resolve(items, targets, policy)
}

fn norm(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

/// `dir/stem (k).ext` for the auto-suffix policy.
fn suffixed(target: &Path, k: u32) -> PathBuf {
    let stem = target.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = target.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    target.with_file_name(format!("{stem} ({k}){ext}"))
}

/// Is `target` taken, from the point of view of moving `source` onto it?
/// A case-only rename (`IMG.jpg` → `img.jpg`) names the same file on a
/// case-insensitive filesystem, so there only an exact directory entry other
/// than the source itself counts.
fn occupied(target: &Path, source: &Path) -> bool {
    if norm(target) != norm(source) {
        return target.exists();
    }
    if target == source {
        return false;
    }
    let Some(name) = target.file_name() else { return false };
    std::fs::read_dir(target.parent().unwrap_or(Path::new("")))
        .map(|mut entries| entries.any(|e| e.is_ok_and(|e| e.file_name() == name)))
        .unwrap_or(false)
}

fn resolve(items: &[BatchFile], targets: Vec<PathBuf>, policy: CollisionPolicy) -> BatchPlan {
    let sources: HashSet<String> = items.iter().map(|i| norm(Path::new(&i.path))).collect();
    let mut taken: HashSet<String> = HashSet::new();
    let mut plan = BatchPlan::default();

    for (item, target) in items.iter().zip(targets) {
        let source = Path::new(&item.path);
        if target == source {
            continue;
        }
        let source_sc = sidecar_path(&item.path);

        let check = |candidate: &Path, taken: &HashSet<String>| -> Option<CollisionReason> {
            let n = norm(candidate);
            if taken.contains(&n) {
                return Some(CollisionReason::DuplicateTarget);
            }
            if n != norm(source) && sources.contains(&n) {
                return Some(CollisionReason::TargetExists);
            }
            if occupied(candidate, source) {
                return Some(CollisionReason::TargetExists);
            }
            let candidate_sc = sidecar_path(&candidate.to_string_lossy());
            occupied(&candidate_sc, &source_sc).then_some(CollisionReason::SidecarExists)
        };

        let mut candidate = target.clone();
        let mut tries = 1;
        let verdict = loop {
            match check(&candidate, &taken) {
                None => break None,
                Some(reason) => match policy {
                    CollisionPolicy::Refuse => break Some(reason),
                    CollisionPolicy::AutoSuffix if tries < MAX_SUFFIX_TRIES => {
                        tries += 1;
                        candidate = suffixed(&target, tries);
                    }
                    CollisionPolicy::AutoSuffix => break Some(reason),
                },
            }
        };
        let to = candidate.to_string_lossy().into_owned();
        match verdict {
            None => {
                taken.insert(norm(&candidate));
                plan.moves.push(PlannedMove { id: item.id, from: item.path.clone(), to });
            }
            Some(reason) => {
                plan.collisions.push(Collision {
                    id: item.id,
                    from: item.path.clone(),
                    to: target.to_string_lossy().into_owned(),
                    reason,
                });
            }
        }
    }
    plan
}

// ---- applying -------------------------------------------------------------

/// Apply `moves`: real file moves with sidecars and catalog rows following.
/// All-or-nothing — see the module docs. Returns the applied moves, which
/// [`revert_batch`] (and the 03-012 undo window) can reverse.
pub fn apply_batch(
    catalog: &mut Catalog,
    moves: &[PlannedMove],
) -> Result<Vec<PlannedMove>, BatchError> {
    let mut applied: Vec<PlannedMove> = Vec::with_capacity(moves.len());
    for mv in moves {
        if let Err(reason) = apply_one(catalog, mv) {
            let mut rolled_back = 0;
            let mut rollback_failures = Vec::new();
            for done in applied.iter().rev() {
                match apply_one(catalog, &done.inverse()) {
                    Ok(()) => rolled_back += 1,
                    Err(err) => rollback_failures.push(format!("{}: {err}", done.to)),
                }
            }
            return Err(BatchError::Aborted {
                failed: mv.clone(),
                reason,
                rolled_back,
                rollback_failures,
            });
        }
        applied.push(mv.clone());
    }
    Ok(applied)
}

/// Undo a batch that [`apply_batch`] returned: every photo goes back to its
/// old path, newest first.
pub fn revert_batch(
    catalog: &mut Catalog,
    applied: &[PlannedMove],
) -> Result<Vec<PlannedMove>, BatchError> {
    let inverse: Vec<PlannedMove> = applied.iter().rev().map(PlannedMove::inverse).collect();
    apply_batch(catalog, &inverse)
}

fn apply_one(catalog: &mut Catalog, mv: &PlannedMove) -> Result<(), String> {
    let from = Path::new(&mv.from);
    let to = Path::new(&mv.to);
    if !from.is_file() {
        return Err("source file is missing".into());
    }
    let Some(dest_dir) = to.parent().filter(|d| d.is_dir()) else {
        return Err("destination folder does not exist".into());
    };
    if occupied(to, from) {
        return Err("a file with the target name already exists".into());
    }
    let (sc_from, sc_to) = (sidecar_path(&mv.from), sidecar_path(&mv.to));
    let has_sidecar = sc_from.is_file();
    if occupied(&sc_to, &sc_from) {
        return Err("a sidecar with the target name already exists".into());
    }

    move_file(from, to).map_err(|e| format!("moving the file: {e}"))?;
    let undo_original = || {
        if let Err(err) = move_file(to, from) {
            tracing::error!(from = %to.display(), to = %from.display(), error = %err,
                "could not put a photo back after a failed batch step");
        }
    };

    if has_sidecar {
        if let Err(err) = move_file(&sc_from, &sc_to) {
            undo_original();
            return Err(format!("moving the sidecar: {err}"));
        }
        // The previous-version backup travels with its sidecar; best effort.
        let (bak_from, bak_to) = (backup_of(&sc_from), backup_of(&sc_to));
        if bak_from.is_file() && !bak_to.exists() {
            if let Err(err) = move_file(&bak_from, &bak_to) {
                tracing::warn!(backup = %bak_from.display(), error = %err, "sidecar backup left behind");
            }
        }
    }

    let undo_all = || {
        if has_sidecar {
            if let Err(err) = move_file(&sc_to, &sc_from) {
                tracing::error!(sidecar = %sc_to.display(), error = %err,
                    "could not put a sidecar back after a failed batch step");
            }
        }
        undo_original();
    };

    let md = match std::fs::metadata(to) {
        Ok(md) => md,
        Err(err) => {
            undo_all();
            return Err(format!("reading the moved file: {err}"));
        }
    };
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64());
    if let Err(err) =
        catalog.relocate_file(mv.id, &mv.to, &dest_dir.to_string_lossy(), md.len() as i64, mtime)
    {
        undo_all();
        return Err(format!("updating the catalog: {err}"));
    }
    Ok(())
}

fn backup_of(sidecar: &Path) -> PathBuf {
    PathBuf::from(format!("{}.fb-bak", sidecar.display()))
}

/// Rename, falling back to copy-then-delete across filesystems. Callers
/// check [`occupied`] first, so nothing existing is overwritten.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Err(err) if err.kind() == std::io::ErrorKind::CrossesDevices => copy_then_delete(from, to),
        other => other,
    }
}

fn copy_then_delete(from: &Path, to: &Path) -> std::io::Result<()> {
    let tmp = PathBuf::from(format!("{}.fb-mv", to.display()));
    let result = (|| {
        std::fs::copy(from, &tmp)?;
        // `copy` does not carry the mtime on every OS; the catalog and the
        // re-association probe both rely on it.
        let mtime = std::fs::metadata(from)?.modified()?;
        std::fs::File::options().write(true).open(&tmp)?.set_modified(mtime)?;
        std::fs::rename(&tmp, to)
    })();
    if let Err(err) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    if let Err(err) = std::fs::remove_file(from) {
        // Keep exactly one copy: the source stays, the new copy goes.
        let _ = std::fs::remove_file(to);
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_device_fallback_keeps_bytes_and_mtime_and_removes_the_source() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a.jpg"), dir.path().join("b.jpg"));
        std::fs::write(&from, b"pixels").unwrap();
        let old = filetime::FileTime::from_unix_time(1_000_000_000, 0);
        filetime::set_file_mtime(&from, old).unwrap();

        copy_then_delete(&from, &to).unwrap();

        assert!(!from.exists());
        assert_eq!(std::fs::read(&to).unwrap(), b"pixels");
        let mtime =
            filetime::FileTime::from_last_modification_time(&std::fs::metadata(&to).unwrap());
        assert_eq!(mtime, old);
        assert!(!dir.path().join("b.jpg.fb-mv").exists(), "no temp file left behind");
    }
}
