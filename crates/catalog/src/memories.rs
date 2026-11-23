//! "On this day" memories (TASK-07-008, PLAN §2 2026 additions): photos
//! captured on today's calendar day in other years. A live, read-only query
//! over `files.taken_at` — nothing is stored, nothing needs rebuilding, no
//! sidecar is touched.

use crate::query::{is_iso_date, CompiledFilters, FileQuery, CAPTURE_TS};
use crate::{
    file_info_row, Catalog, CatalogError, FileInfo, Result, FILE_INFO_COLS, STACK_COLLAPSED,
};

/// Only a date-shaped `taken_at` (EXIF `2023:10:20 …` or ISO `2023-10-20T…`)
/// counts: unlike the Folders view's date, the `mtime` fallback is not a
/// capture date and must not surface a copy made today as a "memory".
const HAS_CAPTURE_DATE: &str =
    "f.taken_at GLOB '[0-9][0-9][0-9][0-9][-:][0-9][0-9][-:][0-9][0-9]*'";

/// One year's worth of memories.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MemoryYear {
    pub year: i32,
    pub files: Vec<FileInfo>,
}

impl Catalog {
    /// Photos taken on `today`'s month/day in any year up to `today`'s,
    /// grouped by year (newest first, chronological inside a year). `today`
    /// is `YYYY-MM-DD`, supplied by the caller so the query stays
    /// deterministic. `query` carries the grid's filters (folders, album,
    /// rating, …), which AND-combine; hidden photos are excluded like in the
    /// grid. Years without photos never appear.
    pub fn memories(&self, query: &FileQuery, today: &str) -> Result<Vec<MemoryYear>> {
        if !is_iso_date(today) {
            return Err(CatalogError::InvalidQuery(format!(
                "today must be YYYY-MM-DD, got {today:?}"
            )));
        }
        let Some(filters) = self.compile_filters(query)? else {
            return Ok(Vec::new());
        };
        let CompiledFilters { conds, mut params, from } = filters;
        let this_year: i64 = today[..4].parse().unwrap_or(i64::MAX);
        params.push(Box::new(today[5..7].to_string()));
        params.push(Box::new(today[8..10].to_string()));
        params.push(Box::new(this_year));

        let sql = format!(
            "SELECT {FILE_INFO_COLS}, substr(f.taken_at, 1, 4) FROM {from} WHERE {HAS_CAPTURE_DATE}{conds}{STACK_COLLAPSED}{} \
             AND substr(f.taken_at, 6, 2) = ? AND substr(f.taken_at, 9, 2) = ? \
             AND CAST(substr(f.taken_at, 1, 4) AS INTEGER) <= ? \
             ORDER BY substr(f.taken_at, 1, 4) DESC, {CAPTURE_TS}, f.path",
            self.not_hidden()
        );
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut rows = stmt.query(refs.as_slice())?;
        let mut years: Vec<MemoryYear> = Vec::new();
        while let Some(row) = rows.next()? {
            let year: i32 = row.get::<_, String>(13)?.parse().unwrap_or(0);
            let file = file_info_row(row)?;
            match years.last_mut() {
                Some(last) if last.year == year => last.files.push(file),
                _ => years.push(MemoryYear { year, files: vec![file] }),
            }
        }
        Ok(years)
    }
}
