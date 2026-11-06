//! The library's single query pipeline (TASK-03-014, PLAN §2 C): live
//! search text, folder selection, album view and the filter buttons / date
//! slider all compile into one SQL `WHERE`, so every combination ANDs
//! consistently and a 50k-photo library never ships rows over IPC just to
//! hide most of them. Everything here is derived, catalog-side data — no
//! sidecar is involved.

use crate::{
    file_info_row, fts_match_expr, Catalog, CatalogError, FileInfo, MapPin, Result, FILE_INFO_COLS,
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
    /// "Has faces" filter (TASK-05-007): photos with at least one detected
    /// or named face row.
    pub has_faces: bool,
    /// "Geotagged" filter (TASK-06-007, PLAN §6 P3): photos with a complete
    /// GPS mirror — the same predicate `geotagged_pins` applies, so grid and
    /// map agree on what counts as geotagged.
    pub geotagged: bool,
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

/// The `WHERE` fragment the grid query (TASK-03-014) and the filtered map-pin
/// query (TASK-06-003) share: search text, folder selection, album view and
/// the filter buttons / date slider AND-combine identically in both, so the
/// map always shows exactly the pins of the view the grid shows.
struct CompiledFilters {
    /// SQL conditions referencing the `f` alias of `files`.
    conds: String,
    params: Vec<Box<dyn rusqlite::ToSql>>,
    /// `FROM` clause producing `files f` (FTS / album joins included).
    from: String,
}

impl Catalog {
    /// Compile the shared filters. `Ok(None)` means "matches nothing"
    /// (unsearchable search text or an empty folder selection) — callers
    /// return an empty result without running SQL.
    fn compile_filters(&self, query: &FileQuery) -> Result<Option<CompiledFilters>> {
        if let Some(cutoff) = &query.taken_since {
            if !is_iso_date(cutoff) {
                return Err(CatalogError::InvalidQuery(format!(
                    "date cutoff must be YYYY-MM-DD, got {cutoff:?}"
                )));
            }
        }
        if query.folders.as_ref().is_some_and(|ids| ids.is_empty()) {
            return Ok(None);
        }

        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let mut conds = String::new();
        let from = match (&query.text, query.album) {
            (Some(text), album) => {
                let expr = fts_match_expr(text);
                if expr.is_empty() {
                    return Ok(None);
                }
                params.push(Box::new(expr));
                conds.push_str(" AND files_fts MATCH ?");
                if let Some(album) = album {
                    params.push(Box::new(album));
                    conds.push_str(
                        " AND f.id IN (SELECT file_id FROM album_files WHERE album_id = ?)",
                    );
                }
                "files_fts JOIN files f ON f.id = files_fts.rowid".to_string()
            }
            (None, Some(album)) => {
                params.push(Box::new(album));
                conds.push_str(" AND af.album_id = ?");
                "album_files af JOIN files f ON f.id = af.file_id".to_string()
            }
            (None, None) => "files f".to_string(),
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
        if query.has_faces {
            conds.push_str(" AND EXISTS (SELECT 1 FROM faces fa WHERE fa.file_id = f.id)");
        }
        // TASK-06-007: complete GPS pair only, plus a hash — the exact
        // predicate `geotagged_pins` uses, so a geotagged-filtered grid and
        // the map's pins are the same set.
        if query.geotagged {
            conds.push_str(
                " AND f.hash IS NOT NULL AND f.id IN (SELECT file_id FROM metadata \
                 WHERE latitude IS NOT NULL AND longitude IS NOT NULL \
                 AND latitude BETWEEN -90.0 AND 90.0 \
                 AND longitude BETWEEN -180.0 AND 180.0)",
            );
        }
        if let Some(cutoff) = &query.taken_since {
            params.push(Box::new(cutoff.clone()));
            conds.push_str(&format!(" AND {FILE_DATE} >= ?"));
        }
        Ok(Some(CompiledFilters { conds, params, from: from.to_string() }))
    }

    /// Grid rows for `query`, at most `limit`: best match first when there is
    /// search text, album order inside an album, otherwise by path. Hidden
    /// photos are excluded unless "show hidden" is on.
    pub fn query_files(&self, query: &FileQuery, limit: usize) -> Result<Vec<FileInfo>> {
        let Some(filters) = self.compile_filters(query)? else {
            return Ok(Vec::new());
        };
        let CompiledFilters { mut conds, mut params, from } = filters;
        let order = match (&query.text, query.album) {
            (Some(_), _) => "rank",
            (None, Some(_)) => "af.position, f.path",
            (None, None) => "f.path",
        };
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

    /// Every non-hidden photo with a complete GPS mirror, for the map's
    /// clustered pins (TASK-06-002). The catalog is a rebuildable cache —
    /// the coordinates mirror the sidecars, the source of truth (§3 rule 2).
    pub fn geotagged_pins(&self) -> Result<Vec<MapPin>> {
        let sql = format!(
            "SELECT f.id, f.filename, f.hash, m.latitude, m.longitude \
             FROM files f JOIN metadata m ON m.file_id = f.id \
             WHERE f.hash IS NOT NULL \
               AND m.latitude IS NOT NULL AND m.longitude IS NOT NULL \
               AND m.latitude BETWEEN -90.0 AND 90.0 \
               AND m.longitude BETWEEN -180.0 AND 180.0{} \
             ORDER BY f.path",
            self.not_hidden()
        );
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let mut pins = Vec::new();
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            pins.push(MapPin {
                id: r.get(0)?,
                filename: r.get(1)?,
                hash: r.get(2)?,
                latitude: r.get(3)?,
                longitude: r.get(4)?,
            });
        }
        Ok(pins)
    }

    /// The geotagged pins of the current view (TASK-06-003): the geotagged
    /// set narrowed by the same filters the grid's `query_files` applies
    /// (search text, folders, album, rating, videos, faces, geotagged
    /// toggle, date cutoff). `stack`/`sort` are grid-only concerns and
    /// ignored — pins have no collapsed stacks and no meaningful order
    /// until clustered client-side.
    pub fn geotagged_pins_filtered(&self, query: &FileQuery) -> Result<Vec<MapPin>> {
        let Some(filters) = self.compile_filters(query)? else {
            return Ok(Vec::new());
        };
        let CompiledFilters { conds, params, from } = filters;
        let sql = format!(
            "SELECT f.id, f.filename, f.hash, m.latitude, m.longitude \
             FROM {from} JOIN metadata m ON m.file_id = f.id \
             WHERE f.hash IS NOT NULL \
               AND m.latitude IS NOT NULL AND m.longitude IS NOT NULL \
               AND m.latitude BETWEEN -90.0 AND 90.0 \
               AND m.longitude BETWEEN -180.0 AND 180.0{conds}{} \
             ORDER BY f.path",
            self.not_hidden()
        );
        let mut stmt = self.conn().prepare_cached(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), |r| {
            Ok(MapPin {
                id: r.get(0)?,
                filename: r.get(1)?,
                hash: r.get(2)?,
                latitude: r.get(3)?,
                longitude: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}
