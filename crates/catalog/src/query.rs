//! The library's single query pipeline (TASK-03-014, PLAN §2 C): live
//! search text, folder selection, album view and the filter buttons / date
//! slider all compile into one SQL `WHERE`, so every combination ANDs
//! consistently and a 50k-photo library never ships rows over IPC just to
//! hide most of them. Everything here is derived, catalog-side data — no
//! sidecar is involved.

use crate::{
    file_info_row, fts_match_expr, Catalog, CatalogError, FileInfo, Result, FILE_INFO_COLS,
    STACK_COLLAPSED,
};

/// Date of a file as `YYYY-MM-DD`: `taken_at` when it is date-shaped (EXIF
/// `2023:05:01 …` or ISO `2023-05-01T…`), else the `mtime` — the same
/// fallback the Folders view uses for its year groups.
const FILE_DATE: &str = "COALESCE(CASE WHEN f.taken_at GLOB \
    '[0-9][0-9][0-9][0-9][-:][0-9][0-9][-:][0-9][0-9]*' \
    THEN replace(substr(f.taken_at, 1, 10), ':', '-') END, date(f.mtime, 'unixepoch'))";

/// Capture instant of a file in Unix seconds: `taken_at` (ISO with or
/// without offset, or EXIF's colon-date form; a naive reading counts as UTC),
/// else the `mtime`. Sorting by the instant keeps photos from cameras in
/// different timezones in true chronological order.
pub(crate) const CAPTURE_TS: &str = "COALESCE(unixepoch(CASE WHEN f.taken_at GLOB \
    '[0-9][0-9][0-9][0-9]:[0-9][0-9]:[0-9][0-9]*' \
    THEN replace(substr(f.taken_at, 1, 10), ':', '-') || substr(f.taken_at, 11) \
    ELSE f.taken_at END), CAST(f.mtime AS INTEGER))";

/// Grid order (TASK-03-019): adjusted capture times show up here, so a
/// merged multi-camera set interleaves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSort {
    /// Best search match, album order inside an album, otherwise by path.
    #[default]
    Path,
    /// Chronological by capture instant; ties keep the `Path` order.
    Capture,
}

/// What the grid asks for. Every field narrows; `Default` is the whole
/// (non-hidden) library, path-ordered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileQuery {
    /// Live search text (FTS over filename, folder, caption, tags). `Some`
    /// with no searchable token matches nothing, like `search_files`.
    pub text: Option<String>,
    /// Folders-view selection; `Some(vec![])` filters everything out.
    pub folders: Option<Vec<i64>>,
    /// Album view; keeps the album's own order when there is no search text.
    pub album: Option<i64>,
    /// Starred filter: rating mirror at least this (5 in star-only mode, 1
    /// when 1–5 ratings are enabled — the caller picks, ADR-0002).
    pub min_rating: Option<u8>,
    /// Videos-only filter (`files.media_type`).
    pub videos_only: bool,
    /// Date-slider cutoff, `YYYY-MM-DD`, inclusive.
    pub taken_since: Option<String>,
    /// Expand one stack (TASK-03-021): its members, top first, instead of
    /// the collapsed grid. Without it every stack shows as its top only.
    pub stack: Option<String>,
    pub sort: FileSort,
}

fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    if ![0, 1, 2, 3, 5, 6, 8, 9].iter().all(|&i| b[i].is_ascii_digit()) {
        return false;
    }
    let month: u32 = s[5..7].parse().unwrap_or(0);
    let day: u32 = s[8..10].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

impl Catalog {
    /// Grid rows for `query`, at most `limit`: best match first when there is
    /// search text, album order inside an album, otherwise by path. Hidden
    /// photos are excluded unless "show hidden" is on.
    pub fn query_files(&self, query: &FileQuery, limit: usize) -> Result<Vec<FileInfo>> {
        if let Some(cutoff) = &query.taken_since {
            if !is_iso_date(cutoff) {
                return Err(CatalogError::InvalidQuery(format!(
                    "date cutoff must be YYYY-MM-DD, got {cutoff:?}"
                )));
            }
        }
        if query.folders.as_ref().is_some_and(|ids| ids.is_empty()) {
            return Ok(Vec::new());
        }

        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let mut conds = String::new();
        let (from, order) = match (&query.text, query.album) {
            (Some(text), album) => {
                let expr = fts_match_expr(text);
                if expr.is_empty() {
                    return Ok(Vec::new());
                }
                params.push(Box::new(expr));
                conds.push_str(" AND files_fts MATCH ?");
                if let Some(album) = album {
                    params.push(Box::new(album));
                    conds.push_str(
                        " AND f.id IN (SELECT file_id FROM album_files WHERE album_id = ?)",
                    );
                }
                ("files_fts JOIN files f ON f.id = files_fts.rowid", "rank")
            }
            (None, Some(album)) => {
                params.push(Box::new(album));
                conds.push_str(" AND af.album_id = ?");
                ("album_files af JOIN files f ON f.id = af.file_id", "af.position, f.path")
            }
            (None, None) => ("files f", "f.path"),
        };
        if let Some(ids) = &query.folders {
            let placeholders = "?,".repeat(ids.len());
            conds
                .push_str(&format!(" AND f.folder_id IN ({})", placeholders.trim_end_matches(',')));
            params.extend(ids.iter().map(|&id| Box::new(id) as Box<dyn rusqlite::ToSql>));
        }
        if let Some(min) = query.min_rating {
            params.push(Box::new(i64::from(min)));
            conds.push_str(" AND f.id IN (SELECT file_id FROM metadata WHERE rating >= ?)");
        }
        if query.videos_only {
            conds.push_str(" AND f.media_type = 'video'");
        }
        if let Some(cutoff) = &query.taken_since {
            params.push(Box::new(cutoff.clone()));
            conds.push_str(&format!(" AND {FILE_DATE} >= ?"));
        }
        let order = match query.sort {
            FileSort::Path => order.to_string(),
            FileSort::Capture => format!("{CAPTURE_TS}, {order}"),
        };
        let order = if query.stack.is_some() {
            format!("(f.stack_role = 'top') DESC, {order}")
        } else {
            order
        };
        match &query.stack {
            Some(id) => {
                params.push(Box::new(id.clone()));
                conds.push_str(" AND f.stack_id = ?");
            }
            None => conds.push_str(STACK_COLLAPSED),
        }
        params.push(Box::new(limit as i64));

        let sql = format!(
            "SELECT {FILE_INFO_COLS} FROM {from} WHERE 1 = 1{conds}{} ORDER BY {order} LIMIT ?",
            self.not_hidden()
        );
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), file_info_row)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}
