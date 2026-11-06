//! Geotag tool (TASK-06-006, PLAN §2 Tools menu "Geotag", §6 P3 Places).
//!
//! One pass applies one location to every target photo — a single photo, a
//! multi-selection, a whole folder or a whole album. Each photo geotags
//! through the regular merge-on-write path (sidecar first, catalog mirror
//! second), so a failure mid-batch leaves earlier photos correctly tagged
//! and later ones untouched. Before the first write the previous GPS value
//! of every target is snapshotted into the catalog's `geotag_undo` table;
//! [`Catalog::undo_geotag`] restores those values (set or clear) through the
//! same merge-on-write path.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::metadata_edit::{GpsEdit, MetadataEdit};
use crate::sync::SyncResult;
use crate::Catalog;

/// Who the tool targets: an explicit selection, or everything in a folder
/// or album (resolved at run time so a folder grows/shrinks between picking
/// and running without the batch going stale).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GeotagTarget {
    /// A struct variant, not a newtype one: serde's internally-tagged
    /// enums cannot deserialize a newtype variant holding a bare sequence
    /// from `{"kind": "ids", "ids": [...]}`.
    Ids {
        ids: Vec<i64>,
    },
    Folder {
        folder_id: i64,
    },
    Album {
        album_id: i64,
    },
}

/// One skipped photo: its data was left untouched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GeotagFailure {
    pub id: i64,
    pub error: String,
}

/// Outcome of one tool run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GeotagToolReport {
    pub applied: usize,
    pub failed: Vec<GeotagFailure>,
    pub cancelled: bool,
}

/// One tool run: the target plus the location to apply. One command
/// argument keeps the IPC surface a single required key (`request`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeotagRequest {
    pub target: GeotagTarget,
    /// Decimal degrees.
    pub latitude: f64,
    /// Decimal degrees.
    pub longitude: f64,
}

impl Catalog {
    /// Run the tool: geotag every target at `latitude`/`longitude` in one
    /// pass, snapshotting each photo's prior GPS for [`Catalog::undo_geotag`].
    /// `progress(done, total)` fires after every photo; cancelling the flag
    /// stops the batch between photos, leaving the processed photos valid.
    pub fn geotag_tool_run(
        &mut self,
        target: &GeotagTarget,
        latitude: f64,
        longitude: f64,
        cancel: &AtomicBool,
        progress: &dyn Fn(usize, usize),
    ) -> SyncResult<GeotagToolReport> {
        let ids = self.resolve_targets(target)?;
        let mut report = GeotagToolReport::default();
        if ids.is_empty() {
            return Ok(report);
        }
        // Coordinates are checked once, up front, so a bad location writes
        // nothing anywhere (same rule as drag-to-geotag).
        let describe =
            |v: f64, axis: &str| format!("{v} is not a valid {axis} (decimal degrees, ±90/±180)");
        if !(-90.0..=90.0).contains(&latitude) || latitude.is_nan() {
            return Err(crate::SyncError::InvalidGps(describe(latitude, "latitude")));
        }
        if !(-180.0..=180.0).contains(&longitude) || longitude.is_nan() {
            return Err(crate::SyncError::InvalidGps(describe(longitude, "longitude")));
        }

        self.snapshot_gps(&ids)?;
        let total = ids.len();
        for (done, &file_id) in ids.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                report.cancelled = true;
                break;
            }
            let edit = MetadataEdit {
                gps: Some(GpsEdit::Set {
                    latitude: format!("{latitude:.7}"),
                    longitude: format!("{longitude:.7}"),
                }),
                ..Default::default()
            };
            match self.edit_metadata(file_id, &edit) {
                Ok(_) => report.applied += 1,
                Err(e) => report.failed.push(GeotagFailure { id: file_id, error: e.to_string() }),
            }
            progress(done + 1, total);
        }
        if report.cancelled && report.applied == 0 {
            // Nothing was written, so the snapshot describes the present and
            // would make a bogus undo; drop it.
            self.conn().execute("DELETE FROM geotag_undo", [])?;
        }
        Ok(report)
    }

    /// Undo the latest tool batch: restore every snapshotted photo to the
    /// GPS it had before (set or clear), then spend the snapshot. Returns
    /// how many photos were restored; `0` when there is nothing to undo.
    pub fn undo_geotag(&mut self, progress: &dyn Fn(usize, usize)) -> SyncResult<usize> {
        let rows: Vec<(i64, Option<(f64, f64)>)> = {
            let conn = self.conn();
            let mut stmt = conn.prepare("SELECT file_id, lat, lon FROM geotag_undo ORDER BY id")?;
            let rows = stmt
                .query_map([], |r| {
                    let id: i64 = r.get(0)?;
                    let lat: Option<f64> = r.get(1)?;
                    let lon: Option<f64> = r.get(2)?;
                    Ok((id, lat.zip(lon)))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let total = rows.len();
        for (done, &(file_id, prior)) in rows.iter().enumerate() {
            let edit = MetadataEdit {
                gps: Some(match prior {
                    Some((lat, lon)) => GpsEdit::Set {
                        latitude: format!("{lat:.7}"),
                        longitude: format!("{lon:.7}"),
                    },
                    None => GpsEdit::Clear,
                }),
                ..Default::default()
            };
            self.edit_metadata(file_id, &edit)?;
            progress(done + 1, total);
        }
        self.conn().execute("DELETE FROM geotag_undo", [])?;
        Ok(total)
    }

    /// Resolve a target to a deterministic, sorted id list.
    fn resolve_targets(&self, target: &GeotagTarget) -> SyncResult<Vec<i64>> {
        let mut ids = match target {
            GeotagTarget::Ids { ids } => ids.clone(),
            GeotagTarget::Folder { folder_id } => {
                let mut stmt = self
                    .conn()
                    .prepare("SELECT f.id FROM files f WHERE f.folder_id = ?1 AND f.hidden = 0")?;
                let ids = stmt
                    .query_map([folder_id], |r| r.get::<_, i64>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                ids
            }
            GeotagTarget::Album { album_id } => self.album_file_ids(*album_id)?,
        };
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    /// Snapshot every target's current sidecar GPS (the source of truth)
    /// into `geotag_undo`, replacing any previous snapshot: undo is one
    /// step, back to before the latest batch.
    fn snapshot_gps(&mut self, ids: &[i64]) -> SyncResult<()> {
        self.conn().execute("DELETE FROM geotag_undo", [])?;
        for &file_id in ids {
            let path: String = self
                .conn()
                .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))
                .map_err(|_| crate::SyncError::UnknownFile(file_id))?;
            let prior = fotobase_metadata::Sidecar::read(&crate::sync::sidecar_path(&path))
                .ok()
                .and_then(|sc| sc.gps());
            self.conn().execute(
                "INSERT INTO geotag_undo (file_id, lat, lon) VALUES (?1, ?2, ?3)",
                rusqlite::params![file_id, prior.map(|p| p.0), prior.map(|p| p.1)],
            )?;
        }
        Ok(())
    }
}
