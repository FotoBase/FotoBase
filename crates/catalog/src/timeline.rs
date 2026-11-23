//! Full-screen timeline (TASK-08-018, PLAN §2 §B): one group per calendar
//! day, oldest first. A live, read-only query over the catalog — nothing is
//! stored, nothing needs rebuilding, no sidecar is touched.

use crate::query::{CompiledFilters, FileQuery, CAPTURE_TS, FILE_DATE};
use crate::{Catalog, Result, STACK_COLLAPSED};

/// One day on the timeline.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TimelineGroup {
    /// `YYYY-MM-DD`: the capture date, or the `mtime` day when undated.
    pub date: String,
    /// Photos captured that day.
    pub count: i64,
    /// Earliest photo of the day: the group's thumbnail, and the photo the
    /// grid reveals when the group is opened.
    pub first_id: i64,
    pub hash: Option<String>,
}

impl Catalog {
    /// Day groups for `query`, oldest first. `query` carries the grid's
    /// filters, which AND-combine exactly as in the grid; hidden photos are
    /// excluded and stacks count as their top only, like the grid. Only ids,
    /// hashes and counts leave the catalog — thumbnails travel through
    /// `thumb://`, never over IPC (§3 rule 3).
    pub fn timeline_groups(&self, query: &FileQuery) -> Result<Vec<TimelineGroup>> {
        let Some(filters) = self.compile_filters(query)? else {
            return Ok(Vec::new());
        };
        let CompiledFilters { conds, params, from } = filters;
        // SQLite takes the bare columns from the row holding MIN(), so each
        // group's id/hash belong to its earliest photo.
        let sql = format!(
            "SELECT {FILE_DATE} AS day, COUNT(*), MIN({CAPTURE_TS}), f.id, f.hash \
             FROM {from} WHERE 1 = 1{conds}{STACK_COLLAPSED}{} \
             GROUP BY day ORDER BY day",
            self.not_hidden()
        );
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), |r| {
            Ok(TimelineGroup {
                date: r.get(0)?,
                count: r.get(1)?,
                first_id: r.get(3)?,
                hash: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}
