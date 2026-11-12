//! FTS + ANN blending (TASK-07-003, PLAN §6 P3 "Semantic search in the
//! live search bar"). FTS5 matches (filename, folder, caption, tags) rank
//! alongside semantic hits via reciprocal-rank fusion: each list votes
//! `1 / (k + rank)`, so an exact text match is never lost in the blend and
//! a photo appearing in both lists rises. Deterministic and unit-testable —
//! ties break by ascending id, never by hash order.

use crate::{file_info_row, Catalog, FileInfo, FileQuery, Result, FILE_INFO_COLS};

/// The RRF smoothing constant (the classic `k = 60`): large enough that
/// rank-1 in one list never drowns rank-2 in the other out of proportion.
const RRF_K: f32 = 60.0;

/// Merge FTS-ranked ids with ANN-ranked ids by reciprocal-rank fusion.
/// A photo's score is the sum of its per-list `1 / (k + rank)` votes
/// (rank starts at 1); equal scores break by ascending id, so the merged
/// order is a pure function of the inputs. Duplicates collapse to one
/// entry; empty `ann` degrades to the FTS order.
pub fn blend_fts_ann(fts: &[i64], ann: &[i64]) -> Vec<i64> {
    let mut scores: std::collections::BTreeMap<i64, f32> = std::collections::BTreeMap::new();
    for ids in [fts, ann] {
        for (rank, id) in ids.iter().enumerate() {
            *scores.entry(*id).or_default() += 1.0 / (RRF_K + rank as f32 + 1.0);
        }
    }
    let mut entries: Vec<(i64, f32)> = scores.into_iter().collect();
    entries.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    entries.into_iter().map(|(id, _)| id).collect()
}

impl Catalog {
    /// The blended grid query (TASK-07-003): FTS matches for `query.text`
    /// and the ANN ids the semantic path produced rank together under
    /// [`blend_fts_ann`], with every non-text filter (folders, album,
    /// rating, videos-only, faces, geotagged, date cutoff, stack collapse,
    /// hidden) applied to **both** candidate lists before blending. The
    /// returned rows follow the blended order; no image bytes here (§3
    /// rule 3).
    pub fn query_files_semantic(
        &self,
        query: &FileQuery,
        ann_ids: &[i64],
        limit: usize,
    ) -> Result<Vec<FileInfo>> {
        // The non-text filters, compiled with no search text, define the
        // candidate set both lists are narrowed to.
        let mut no_text = query.clone();
        no_text.text = None;
        let filters = match self.compile_filters(&no_text)? {
            Some(f) => f,
            None => return Ok(Vec::new()),
        };
        let sql = format!(
            "SELECT f.id FROM {} WHERE 1 = 1{}{}",
            filters.from,
            filters.conds,
            self.not_hidden()
        );
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = filters.params.iter().map(|p| p.as_ref()).collect();
        let allowed: std::collections::HashSet<i64> =
            stmt.query_map(refs.as_slice(), |r| r.get(0))?.flatten().collect();

        // ANN hits keep their distance order; FTS keeps its rank order.
        let ann: Vec<i64> = ann_ids.iter().copied().filter(|id| allowed.contains(id)).collect();
        let fts: Vec<i64> = match &query.text {
            Some(text) => self
                .search_files(text, usize::MAX)?
                .into_iter()
                .filter(|id| allowed.contains(id))
                .collect(),
            None => Vec::new(),
        };
        let blended = blend_fts_ann(&fts, &ann);
        if blended.is_empty() {
            return Ok(Vec::new());
        }

        // Fetch the grid rows for the blended ids, then restore the blend
        // order (SQL `IN` has no order of its own).
        let placeholders = "?,".repeat(blended.len()).trim_end_matches(',').to_string();
        let sql = format!(
            "SELECT {FILE_INFO_COLS} FROM files f WHERE f.id IN ({placeholders}){}",
            self.not_hidden()
        );
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let rows: Vec<FileInfo> = stmt
            .query_map(rusqlite::params_from_iter(blended.iter()), file_info_row)?
            .flatten()
            .collect();
        let mut by_id: std::collections::HashMap<i64, FileInfo> =
            rows.into_iter().map(|r| (r.id, r)).collect();
        Ok(blended.into_iter().filter_map(|id| by_id.remove(&id)).take(limit).collect())
    }
}
