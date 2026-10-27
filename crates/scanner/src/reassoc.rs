//! Move/rename re-association (TASK-03-003, 00-010 design §4).
//!
//! When a file is renamed or moved outside Fotobase, the next scan finds a
//! new-looking path. Instead of treating it as a new photo, the scanner
//! matches it against the missing-file records snapshotted when the old path
//! vanished, in tiers: (size + mtime window) → (lazy content hash). Hashing
//! stays lazy (PLAN §5): the new file's hash is computed on demand only when
//! the cheap probe leaves doubt. A confirmed match re-creates the catalog row
//! with the snapshotted user data and the sidecar follows the original.
//! Ambiguous matches (two byte-identical candidates) are surfaced, never
//! silently resolved.

use std::path::Path;

use fotobase_catalog::{sidecar_path, Catalog, MissingFile};
use fotobase_metadata::Sidecar;

use crate::import::content_hash;

/// mtime tolerance for the cheap probe tier. A rename/move preserves mtime
/// (same filesystem); anything outside this window needs hash confirmation.
const MTIME_WINDOW_SECS: f64 = 2.0;

/// Outcome of matching one new-looking path against the missing-file set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReassocOutcome {
    /// The missing record with this id matches; re-associate.
    Match(i64),
    /// `n` byte-identical candidates — surfaced, never silently resolved
    /// (00-010 §4.4).
    Ambiguous(usize),
    /// A genuinely new file.
    NoMatch,
}

/// True when the file's mtime is close enough to count as "unchanged".
fn in_window(record_mtime: f64, file_mtime: f64) -> bool {
    (record_mtime - file_mtime).abs() <= MTIME_WINDOW_SECS
}

/// Tiered matcher. `hash` is called lazily — only when the cheap probe leaves
/// doubt — and returns the new file's content hash (`None` if unreadable).
/// Candidates must already share the file's size and media type.
pub fn match_missing(
    candidates: &[MissingFile],
    size: i64,
    mtime: f64,
    media_type: &str,
    hash: &dyn Fn() -> Option<String>,
) -> ReassocOutcome {
    let candidates: Vec<&MissingFile> =
        candidates.iter().filter(|c| c.size == size && c.media_type == media_type).collect();
    if candidates.is_empty() {
        return ReassocOutcome::NoMatch;
    }

    // Tier 1: size + mtime window. A sole candidate whose hash is unknown
    // (never backfilled) is accepted on the window alone — a rename/move
    // preserves mtime, and there is nothing stronger to compare against.
    let in_window: Vec<&&MissingFile> =
        candidates.iter().filter(|c| in_window(c.mtime, mtime)).collect();
    if in_window.len() == 1 {
        let c = in_window[0];
        match &c.hash {
            None => return ReassocOutcome::Match(c.id),
            Some(h) => {
                if hash().as_deref() == Some(h.as_str()) {
                    return ReassocOutcome::Match(c.id);
                }
                // Content differs → fall through to the hash tier over all
                // candidates (the true match may be out of the window).
            }
        }
    }

    // Tier 2: content hash is authoritative.
    let Some(new_hash) = hash() else { return ReassocOutcome::NoMatch };
    let by_hash: Vec<&&MissingFile> =
        candidates.iter().filter(|c| c.hash.as_deref() == Some(new_hash.as_str())).collect();
    match by_hash.len() {
        1 => ReassocOutcome::Match(by_hash[0].id),
        n if n > 1 => ReassocOutcome::Ambiguous(n),
        _ => ReassocOutcome::NoMatch,
    }
}

/// The sidecar follows the original (00-010 §4.3): after a confirmed
/// re-association, move the old sidecar to the new location. If a sidecar
/// already exists at the destination, ours wins on the fields we own and the
/// destination's previous version is backed up per the merge-on-write
/// discipline (`Sidecar::write` does the `.fb-bak` backup + atomic replace).
pub fn follow_sidecar(old_original: &str, new_original: &str) {
    let old_sc = sidecar_path(old_original);
    if !old_sc.exists() {
        return;
    }
    let new_sc = sidecar_path(new_original);
    if !new_sc.exists() {
        if let Err(err) = std::fs::rename(&old_sc, &new_sc) {
            tracing::warn!(from = %old_sc.display(), to = %new_sc.display(), error = %err,
                "sidecar follow failed; user data stays on the old sidecar");
        }
        return;
    }
    // Destination exists: merge — base is the destination packet (its unknown
    // fields survive), our owned fields win.
    let ours = match Sidecar::read(&old_sc) {
        Ok(sc) => sc,
        Err(err) => {
            tracing::warn!(sidecar = %old_sc.display(), error = %err, "unreadable old sidecar");
            return;
        }
    };
    let mut dest = match Sidecar::read(&new_sc) {
        Ok(sc) => sc,
        Err(err) => {
            tracing::warn!(sidecar = %new_sc.display(), error = %err, "unreadable destination sidecar");
            return;
        }
    };
    if let Some(r) = ours.rating() {
        dest.set_rating(r);
    }
    if let Some(c) = ours.caption() {
        dest.set_caption(&c);
    }
    let tags = ours.tags();
    if !tags.is_empty() {
        dest.set_tags(tags);
    }
    if let Some((lat, lon)) = ours.gps() {
        dest.set_gps(lat, lon);
    }
    if let Err(err) = dest.write(&new_sc) {
        tracing::warn!(sidecar = %new_sc.display(), error = %err, "sidecar merge-on-follow failed");
        return;
    }
    let _ = std::fs::remove_file(&old_sc);
}

/// Try to re-associate one new-looking walked file against the missing-file
/// set. On a confirmed match the catalog row is re-created with the
/// snapshotted user data and the sidecar follows the original.
pub fn try_reassociate(
    catalog: &mut Catalog,
    path: &str,
    dir: &str,
    size: i64,
    mtime: f64,
    media_type: &str,
) -> fotobase_catalog::Result<ReassocOutcome> {
    let candidates = catalog.missing_with_size(size, media_type)?;
    let full_path = path.to_string();
    let outcome = match_missing(&candidates, size, mtime, media_type, &|| {
        content_hash(Path::new(&full_path)).ok()
    });
    match outcome {
        ReassocOutcome::Match(missing_id) => {
            let reassociated =
                catalog.reassociate_file(missing_id, path, dir, size, mtime)?.is_some();
            if reassociated {
                follow_sidecar(&candidates.iter().find(|c| c.id == missing_id).unwrap().path, path);
                Ok(ReassocOutcome::Match(missing_id))
            } else {
                Ok(ReassocOutcome::NoMatch)
            }
        }
        ReassocOutcome::Ambiguous(n) => {
            tracing::warn!(
                file = %path,
                candidates = n,
                "ambiguous re-association candidates; file added as new, no silent merge"
            );
            Ok(ReassocOutcome::Ambiguous(n))
        }
        ReassocOutcome::NoMatch => Ok(ReassocOutcome::NoMatch),
    }
}
