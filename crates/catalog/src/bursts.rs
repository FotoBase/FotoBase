//! Burst grouping and AI culling support in the catalog (TASK-07-006, PLAN
//! §6 P3). Bursts cluster by capture-time gaps with the same camera; the
//! suggested best shot ranks by the derived quality score in
//! `photo_quality` (PLAN §5 derived data — never a sidecar field).
//!
//! User data flows only through the EPIC-03 stacking path
//! (`fb:StackId` / `fb:StackRole` sidecars): stacking a burst with the
//! user's preferred shot first is the "promote" action, and a catalog
//! rebuild never recomputes that pick (PLAN §3 rule 2).

use std::collections::BTreeMap;

use rusqlite::OptionalExtension;
use serde::Serialize;

use crate::query::CAPTURE_TS;
use crate::{Catalog, Result};

/// Default burst gap: consecutive frames of one burst are usually well under
/// a second apart; 2 s separates real bursts from a photographer pausing.
pub const DEFAULT_BURST_GAP_SECS: i64 = 2;

/// `files.quality_scanned` marker values (the phash/embed sentinel pattern).
const SCANNED_DONE: i64 = 1;
const SCANNED_FAILED: i64 = 2;

/// What burst grouping needs of one photo (pure, testable without a catalog).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BurstCandidate {
    pub id: i64,
    /// Capture instant in Unix seconds (the query module's capture-time rule).
    pub captured: i64,
    /// EXIF Make/Model mirror; bursts only form within one camera.
    pub camera: Option<String>,
}

/// A burst worth offering as a stack (PLAN §6 P2a "burst stacks grouped
/// under one thumbnail"). `file_ids` is best-first; nothing is stacked until
/// the user accepts, and accepting with a different shot first promotes it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BurstSuggestion {
    /// Ranked members, the suggested best first.
    pub file_ids: Vec<i64>,
    pub filenames: Vec<String>,
    /// The suggested best shot (the user can promote any member instead).
    pub best_file_id: i64,
}

/// Score used for ranking when a photo has none yet (an un-scored frame
/// never outranks a scored one; among unscored frames capture order wins).
const NO_SCORE: f64 = -1.0;

/// Pure burst grouping over `candidates` (any order): sort by camera, then
/// capture time; a contiguous run whose consecutive gaps are all ≤
/// `max_gap_secs` is one burst. Photos without an EXIF camera never group
/// (an unknown camera cannot prove two frames belong to one burst); runs of
/// a single photo are not suggestions. Members rank by the derived quality
/// `scores` (best first), ties and unscored frames by capture order.
pub fn group_bursts(
    candidates: &[BurstCandidate],
    max_gap_secs: i64,
    scores: &BTreeMap<i64, f64>,
) -> Vec<BurstSuggestion> {
    let mut sorted: Vec<(&BurstCandidate, String)> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            // A unique camera key per unknown-camera photo keeps it out of
            // any run.
            (c, c.camera.clone().unwrap_or_else(|| format!("\0unknown-{i}\0")))
        })
        .collect();
    sorted.sort_by(|a, b| (&a.1, a.0.captured, a.0.id).cmp(&(&b.1, b.0.captured, b.0.id)));

    let mut groups = Vec::new();
    let mut run: Vec<(&BurstCandidate, String)> = Vec::new();
    let flush = |run: &mut Vec<(&BurstCandidate, String)>, groups: &mut Vec<BurstSuggestion>| {
        if run.len() < 2 {
            run.clear();
            return;
        }
        let mut members: Vec<&BurstCandidate> = run.drain(..).map(|(c, _)| c).collect();
        members.sort_by(|a, b| {
            let sa = scores.get(&a.id).copied().unwrap_or(NO_SCORE);
            let sb = scores.get(&b.id).copied().unwrap_or(NO_SCORE);
            sb.partial_cmp(&sa)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| (a.captured, a.id).cmp(&(b.captured, b.id)))
        });
        let best = members[0].id;
        groups.push(BurstSuggestion {
            file_ids: members.iter().map(|c| c.id).collect(),
            filenames: Vec::new(),
            best_file_id: best,
        });
    };
    for (c, key) in sorted {
        if let Some((prev, prev_key)) = run.last() {
            if *prev_key != key || c.captured - prev.captured > max_gap_secs {
                flush(&mut run, &mut groups);
            }
        }
        run.push((c, key));
    }
    flush(&mut run, &mut groups);
    groups
}

impl Catalog {
    /// Store one photo's derived quality score (clobbering any previous
    /// pass's value — the latest pass wins).
    pub fn store_quality_score(&mut self, file_id: i64, score: f64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO photo_quality (file_id, score) VALUES (?1, ?2)
             ON CONFLICT(file_id) DO UPDATE SET score = excluded.score",
            rusqlite::params![file_id, score],
        )?;
        Ok(())
    }

    /// The stored derived score of one photo, `None` before its first pass
    /// (or after a rebuild, until the next pass re-derives it).
    pub fn quality_score(&self, file_id: i64) -> Result<Option<f64>> {
        let score: Option<f64> = self
            .conn
            .query_row("SELECT score FROM photo_quality WHERE file_id = ?1", [file_id], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(score)
    }

    /// Images the quality pass has not processed yet (the embed-scanned
    /// sentinel pattern: 0 pending / 1 done / 2 failed), oldest first.
    pub fn quality_candidates(&self) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, path FROM files \
             WHERE media_type = 'image' AND quality_scanned = 0 ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.flatten().collect())
    }

    /// Set the processed marker: `true` = done, `false` = failed (parks the
    /// row so one undecodable photo neither blocks the pass forever nor
    /// inflates later counts).
    pub fn mark_quality_scanned(&mut self, file_id: i64, ok: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE files SET quality_scanned = ?1 WHERE id = ?2",
            rusqlite::params![if ok { SCANNED_DONE } else { SCANNED_FAILED }, file_id],
        )?;
        Ok(())
    }

    /// Clear the processed marker so a later pass retries a file. Explicit —
    /// nothing retries automatically.
    pub fn reset_quality_scan(&self, file_id: i64) -> Result<()> {
        self.conn.execute("UPDATE files SET quality_scanned = 0 WHERE id = ?1", [file_id])?;
        Ok(())
    }

    /// Bursts worth offering to stack: unstacked photos only, so accepting
    /// (or declining) a suggestion never re-offers the same photos.
    pub fn burst_suggestions(&self, max_gap_secs: i64) -> Result<Vec<BurstSuggestion>> {
        let sql = format!(
            "SELECT f.id, {CAPTURE_TS}, m.camera, f.filename
             FROM files f LEFT JOIN metadata m ON m.file_id = f.id
             WHERE f.stack_id IS NULL AND f.media_type = 'image'
             ORDER BY m.camera, 2, f.id"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut candidates = Vec::new();
        let mut names: BTreeMap<i64, String> = BTreeMap::new();
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for (id, captured, camera, filename) in rows.flatten() {
            names.insert(id, filename);
            candidates.push(BurstCandidate { id, captured, camera });
        }

        let mut scores: BTreeMap<i64, f64> = BTreeMap::new();
        {
            let mut stmt = self.conn.prepare_cached("SELECT file_id, score FROM photo_quality")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            for (id, score) in rows.flatten() {
                scores.insert(id, score);
            }
        }

        let mut suggestions = group_bursts(&candidates, max_gap_secs, &scores);
        for s in &mut suggestions {
            s.filenames = s.file_ids.iter().filter_map(|id| names.get(id).cloned()).collect();
        }
        Ok(suggestions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: i64, captured: i64, camera: Option<&str>) -> BurstCandidate {
        BurstCandidate { id, captured, camera: camera.map(str::to_string) }
    }

    #[test]
    fn empty_input_yields_no_groups() {
        assert!(group_bursts(&[], 2, &BTreeMap::new()).is_empty());
    }

    #[test]
    fn a_single_photo_run_is_not_a_suggestion() {
        let g = group_bursts(&[cand(1, 0, Some("A"))], 2, &BTreeMap::new());
        assert!(g.is_empty());
    }

    #[test]
    fn an_unknown_camera_never_groups() {
        let g = group_bursts(
            &[cand(1, 0, None), cand(2, 1, None), cand(3, 1, Some("A"))],
            2,
            &BTreeMap::new(),
        );
        // Without EXIF the camera cannot prove two frames are one burst.
        assert!(g.is_empty());
    }
}
