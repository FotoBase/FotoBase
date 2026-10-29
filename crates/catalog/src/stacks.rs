//! Stacking (TASK-03-021, PLAN §6 P2a, ADR-0006): RAW+JPEG pairs, bursts and
//! edited versions grouped under one thumbnail. Membership lives in every
//! member's own sidecar (`fb:StackId` + `fb:StackRole`); the catalog's
//! `files.stack_id` / `stack_role` are a derived index, rebuilt from the
//! sidecars. Stacking never moves or rewrites an original, and unstacking
//! gives back exactly the photos that went in.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::OptionalExtension;
use serde::Serialize;

use fotobase_metadata::{StackMembership, StackRole};

use crate::query::CAPTURE_TS;
use crate::sync::{SyncError, SyncResult};
use crate::{file_info_row, Catalog, FileInfo, Result, FILE_INFO_COLS};

/// Default for the pair detector's timestamp window. Camera clocks drift
/// and a RAW+JPEG pair from one shutter press is normally identical, so a
/// few seconds separate real pairs from a wrapped file counter.
pub const DEFAULT_PAIR_WINDOW_SECS: i64 = 5;

/// Extensions that mark the RAW half of a pair (the scanner's RAW list).
const RAW_EXTS: &[&str] = &["dng", "cr2", "cr3", "nef", "arw", "raf", "orf", "rw2"];

/// Characters that may separate an original's stem from an edit's suffix:
/// `beach.jpg` → `beach-edit.jpg`, `beach_v2.jpg`, `beach (edited).jpg`.
const EDIT_SEPARATORS: &[char] = &['-', '_', ' ', '('];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StackKind {
    RawJpeg,
    Edited,
}

/// A group the detector offers to stack. The user decides: nothing is
/// stacked until the ids are handed to [`Catalog::stack_photos`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StackSuggestion {
    pub kind: StackKind,
    /// Proposed order: the top first.
    pub file_ids: Vec<i64>,
    pub filenames: Vec<String>,
}

/// What the pair detector needs of one photo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: i64,
    pub folder_id: i64,
    pub filename: String,
    /// Capture instant in Unix seconds.
    pub captured: i64,
    /// Carries `fb:Recipe`: an edited version.
    pub edited: bool,
}

fn split_name(filename: &str) -> (String, String) {
    match filename.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_lowercase(), ext.to_lowercase()),
        _ => (filename.to_lowercase(), String::new()),
    }
}

fn is_raw(filename: &str) -> bool {
    RAW_EXTS.contains(&split_name(filename).1.as_str())
}

/// Pure pair/edit detection over `candidates` (any order). RAW+JPEG pairs:
/// same folder, same stem (case-insensitive), at least one RAW and one
/// non-RAW file captured within `window_secs` of the first RAW. Edited
/// versions: a file carrying a recipe whose stem extends an unedited
/// file's stem. The non-RAW file leads a pair (it is what renders); an
/// original leads its edits.
pub fn detect_groups(candidates: &[Candidate], window_secs: i64) -> Vec<StackSuggestion> {
    let mut sorted: Vec<&Candidate> = candidates.iter().collect();
    sorted.sort_by(|a, b| (a.folder_id, &a.filename).cmp(&(b.folder_id, &b.filename)));

    let mut by_stem: BTreeMap<(i64, String), Vec<&Candidate>> = BTreeMap::new();
    for c in &sorted {
        by_stem.entry((c.folder_id, split_name(&c.filename).0)).or_default().push(c);
    }

    let mut groups: Vec<(StackKind, Vec<&Candidate>)> = Vec::new();
    let mut used: BTreeSet<i64> = BTreeSet::new();
    for same_stem in by_stem.values() {
        let Some(anchor) = same_stem.iter().find(|c| is_raw(&c.filename)) else { continue };
        let near: Vec<&Candidate> = same_stem
            .iter()
            .filter(|c| (c.captured - anchor.captured).abs() <= window_secs)
            .copied()
            .collect();
        let (raws, others): (Vec<_>, Vec<_>) = near.into_iter().partition(|c| is_raw(&c.filename));
        if others.is_empty() {
            continue;
        }
        let members: Vec<&Candidate> = others.into_iter().chain(raws).collect();
        used.extend(members.iter().map(|c| c.id));
        groups.push((StackKind::RawJpeg, members));
    }

    for edit in sorted.iter().filter(|c| c.edited) {
        if used.contains(&edit.id) {
            continue;
        }
        let (edit_stem, _) = split_name(&edit.filename);
        let original = sorted
            .iter()
            .filter(|o| {
                o.folder_id == edit.folder_id && !o.edited && o.id != edit.id && {
                    let stem = split_name(&o.filename).0;
                    edit_stem
                        .strip_prefix(stem.as_str())
                        .is_some_and(|rest| rest.starts_with(|ch| EDIT_SEPARATORS.contains(&ch)))
                }
            })
            .max_by_key(|o| split_name(&o.filename).0.len());
        let Some(original) = original else { continue };
        used.insert(edit.id);
        match groups.iter_mut().find(|(_, g)| g.iter().any(|m| m.id == original.id)) {
            Some((_, group)) => group.push(edit),
            None => {
                used.insert(original.id);
                groups.push((StackKind::Edited, vec![original, edit]));
            }
        }
    }

    groups
        .into_iter()
        .map(|(kind, members)| StackSuggestion {
            kind,
            file_ids: members.iter().map(|c| c.id).collect(),
            filenames: members.iter().map(|c| c.filename.clone()).collect(),
        })
        .collect()
}

impl Catalog {
    /// Groups worth offering to stack: unstacked photos only, so accepting
    /// a suggestion (or declining it) never re-offers the same photos.
    pub fn stack_suggestions(&self, window_secs: i64) -> Result<Vec<StackSuggestion>> {
        let sql = format!(
            "SELECT f.id, f.folder_id, f.filename, {CAPTURE_TS}, f.has_recipe
             FROM files f WHERE f.stack_id IS NULL AND f.media_type = 'image'"
        );
        let mut stmt = self.conn().prepare(&sql)?;
        let candidates = stmt
            .query_map([], |r| {
                Ok(Candidate {
                    id: r.get(0)?,
                    folder_id: r.get(1)?,
                    filename: r.get(2)?,
                    captured: r.get(3)?,
                    edited: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(detect_groups(&candidates, window_secs))
    }

    /// The photos of one stack, top first, then by path.
    pub fn stack_members(&self, stack_id: &str) -> Result<Vec<FileInfo>> {
        let mut stmt = self.conn().prepare_cached(&format!(
            "SELECT {FILE_INFO_COLS} FROM files f WHERE f.stack_id = ?1
             ORDER BY (f.stack_role = 'top') DESC, f.path"
        ))?;
        let rows = stmt.query_map([stack_id], file_info_row)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Stack `file_ids` together and return the stack id. The photos'
    /// own sidecars are the only thing written.
    ///
    /// - None of them stacked yet: a new stack; the first id is the top.
    /// - Some already in one stack: the others join it as members and its
    ///   top stays (stacking an edit under its original's stack).
    /// - Members of two different stacks: refused, nothing written.
    ///
    /// A failed sidecar write undoes the ones already written.
    pub fn stack_photos(&mut self, file_ids: &[i64]) -> SyncResult<String> {
        let mut ids: Vec<i64> = Vec::new();
        for id in file_ids {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
        if ids.len() < 2 {
            return Err(SyncError::StackTooSmall);
        }
        let mut existing: Vec<(i64, Option<String>)> = Vec::new();
        for id in &ids {
            let stack_id: Option<Option<String>> = self
                .conn()
                .query_row("SELECT stack_id FROM files WHERE id = ?1", [id], |r| r.get(0))
                .optional()?;
            existing.push((*id, stack_id.ok_or(SyncError::UnknownFile(*id))?));
        }
        let joined: BTreeSet<&String> = existing.iter().filter_map(|(_, s)| s.as_ref()).collect();
        if joined.len() > 1 {
            return Err(SyncError::StackConflict);
        }

        let (stack_id, changes) = match joined.into_iter().next() {
            Some(stack_id) => {
                let changes = existing
                    .iter()
                    .filter(|(_, s)| s.is_none())
                    .map(|(id, _)| {
                        (*id, StackMembership { id: stack_id.clone(), role: StackRole::Member })
                    })
                    .collect::<Vec<_>>();
                (stack_id.clone(), changes)
            }
            None => {
                let stack_id = uuid::Uuid::new_v4().to_string();
                let changes = ids
                    .iter()
                    .enumerate()
                    .map(|(i, id)| {
                        let role = if i == 0 { StackRole::Top } else { StackRole::Member };
                        (*id, StackMembership { id: stack_id.clone(), role })
                    })
                    .collect();
                (stack_id, changes)
            }
        };
        let changes: Vec<(i64, Option<StackMembership>)> =
            changes.into_iter().map(|(id, m)| (id, Some(m))).collect();
        self.write_memberships(&changes)?;
        Ok(stack_id)
    }

    /// Make `file_id` the top of its stack; the old top becomes a member.
    pub fn set_stack_top(&mut self, file_id: i64) -> SyncResult<()> {
        let (stack_id, role): (Option<String>, Option<String>) = self
            .conn()
            .query_row("SELECT stack_id, stack_role FROM files WHERE id = ?1", [file_id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?
            .ok_or(SyncError::UnknownFile(file_id))?;
        let stack_id = stack_id.ok_or_else(|| SyncError::UnknownStack(String::new()))?;
        if role.as_deref() == Some("top") {
            return Ok(());
        }
        let mut changes: Vec<(i64, Option<StackMembership>)> = Vec::new();
        for member in self.member_ids(&stack_id)? {
            if member != file_id && self.role_of(member)? == Some(StackRole::Top) {
                changes.push((
                    member,
                    Some(StackMembership { id: stack_id.clone(), role: StackRole::Member }),
                ));
            }
        }
        changes.push((file_id, Some(StackMembership { id: stack_id, role: StackRole::Top })));
        self.write_memberships(&changes)
    }

    /// Dissolve a stack: every member's `fb:StackId` / `fb:StackRole` goes,
    /// nothing else about the photos changes.
    pub fn unstack(&mut self, stack_id: &str) -> SyncResult<()> {
        let members = self.member_ids(stack_id)?;
        if members.is_empty() {
            return Err(SyncError::UnknownStack(stack_id.to_string()));
        }
        let changes: Vec<(i64, Option<StackMembership>)> =
            members.into_iter().map(|id| (id, None)).collect();
        self.write_memberships(&changes)
    }

    fn member_ids(&self, stack_id: &str) -> Result<Vec<i64>> {
        let mut stmt =
            self.conn().prepare_cached("SELECT id FROM files WHERE stack_id = ?1 ORDER BY path")?;
        let rows = stmt.query_map([stack_id], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    fn role_of(&self, file_id: i64) -> Result<Option<StackRole>> {
        let role: Option<Option<String>> = self
            .conn()
            .query_row("SELECT stack_role FROM files WHERE id = ?1", [file_id], |r| r.get(0))
            .optional()?;
        Ok(role.flatten().map(|r| StackRole::from_text(&r)))
    }

    /// One merge-on-write per photo (00-010 §3: sidecar first, catalog
    /// mirror second). A stack op writes N sidecars (ADR-0006), so a
    /// failure part-way puts back the memberships already written.
    fn write_memberships(&mut self, changes: &[(i64, Option<StackMembership>)]) -> SyncResult<()> {
        let mut done: Vec<(i64, Option<StackMembership>)> = Vec::new();
        for (id, membership) in changes {
            let mut previous = None;
            let result = self.mutate_user_data(*id, |sc| {
                previous = sc.stack();
                match membership {
                    Some(m) => sc.set_stack(m),
                    None => sc.clear_stack(),
                }
            });
            if let Err(err) = result {
                for (done_id, before) in done.into_iter().rev() {
                    let restored = self.mutate_user_data(done_id, |sc| match &before {
                        Some(m) => sc.set_stack(m),
                        None => sc.clear_stack(),
                    });
                    if let Err(undo) = restored {
                        tracing::warn!(file_id = done_id, error = %undo, "could not undo a stack write");
                    }
                }
                return Err(err);
            }
            done.push((*id, previous));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: i64, filename: &str, captured: i64) -> Candidate {
        Candidate { id, folder_id: 1, filename: filename.into(), captured, edited: false }
    }

    #[test]
    fn matches_fixture_raw_jpeg_stems_regardless_of_case_or_order() {
        let found = detect_groups(
            &[cand(1, "IMG_0001.CR2", 100), cand(2, "img_0001.jpg", 101), cand(3, "x.jpg", 100)],
            5,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file_ids, [2, 1]);
    }

    #[test]
    fn rejects_the_same_stem_at_a_different_time() {
        let found =
            detect_groups(&[cand(1, "IMG_0001.CR2", 0), cand(2, "IMG_0001.JPG", 86_400)], 5);
        assert!(found.is_empty());
    }

    #[test]
    fn a_stem_that_only_prefixes_another_is_not_a_pair() {
        let found = detect_groups(&[cand(1, "IMG_1.CR2", 0), cand(2, "IMG_10.JPG", 0)], 5);
        assert!(found.is_empty());
    }

    #[test]
    fn an_edit_joins_the_raw_pair_of_its_original() {
        let mut edit = cand(3, "IMG_1-edit.jpg", 0);
        edit.edited = true;
        let found = detect_groups(&[cand(1, "IMG_1.CR2", 0), cand(2, "IMG_1.JPG", 0), edit], 5);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, StackKind::RawJpeg);
        assert_eq!(found[0].file_ids, [2, 1, 3]);
    }

    #[test]
    fn the_longest_matching_stem_is_the_original() {
        let mut edit = cand(3, "trip-beach-edit.jpg", 0);
        edit.edited = true;
        let found = detect_groups(&[cand(1, "trip.jpg", 0), cand(2, "trip-beach.jpg", 0), edit], 5);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file_ids, [2, 3]);
    }
}
