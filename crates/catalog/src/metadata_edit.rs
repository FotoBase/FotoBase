//! Info-pane metadata editing (TASK-03-020, PLAN §6 P2): capture date, GPS,
//! artist and copyright of one photo. Everything the user types is checked
//! first; only a fully valid edit reaches the sidecar, in a single
//! merge-on-write (00-010 §3: sidecar first, catalog mirror second). A
//! field left as `None` is not touched, so an edit of one field never
//! rewrites another.

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use fotobase_metadata::{parse_coordinate, CaptureTime, GpsAxis};

use crate::sync::SyncResult;
use crate::Catalog;

/// What to change on one photo; `None` leaves a field alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct MetadataEdit {
    /// Capture time, ISO-8601 or EXIF text. Without a UTC offset the photo's
    /// current offset is kept; a date cannot be cleared, only replaced.
    pub taken_at: Option<String>,
    pub gps: Option<GpsEdit>,
    /// Blank clears `dc:creator`.
    pub artist: Option<String>,
    /// Blank clears `dc:rights`.
    pub copyright: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GpsEdit {
    /// Decimal degrees or DMS, as typed.
    Set {
        latitude: String,
        longitude: String,
    },
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataField {
    TakenAt,
    Latitude,
    Longitude,
}

/// One rejected input, for display next to its field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldError {
    pub field: MetadataField,
    pub message: String,
}

/// An edit that passed validation.
struct Checked {
    taken_at: Option<CaptureTime>,
    gps: Option<Option<(f64, f64)>>,
}

impl MetadataEdit {
    fn check(&self, current_offset: Option<i32>) -> Result<Checked, Vec<FieldError>> {
        let mut errors = Vec::new();
        let mut reject = |field, message: String| errors.push(FieldError { field, message });

        let taken_at = self.taken_at.as_deref().and_then(|text| match CaptureTime::parse(text) {
            None => {
                reject(
                    MetadataField::TakenAt,
                    "Enter a real date and time, like 2024-06-01 12:00:00.".into(),
                );
                None
            }
            Some(time) => match (time.offset_minutes(), current_offset) {
                (None, Some(offset)) => time.with_offset_keeping_wall_clock(offset),
                _ => Some(time),
            },
        });

        let gps = self.gps.as_ref().and_then(|edit| match edit {
            GpsEdit::Clear => Some(None),
            GpsEdit::Set { latitude, longitude } => {
                let lat = parse_coordinate(latitude, GpsAxis::Latitude);
                let lon = parse_coordinate(longitude, GpsAxis::Longitude);
                match (lat, lon) {
                    (Ok(lat), Ok(lon)) => Some(Some((lat, lon))),
                    (lat, lon) => {
                        if let Err(e) = lat {
                            reject(MetadataField::Latitude, e.to_string());
                        }
                        if let Err(e) = lon {
                            reject(MetadataField::Longitude, e.to_string());
                        }
                        None
                    }
                }
            }
        });

        if errors.is_empty() {
            Ok(Checked { taken_at, gps })
        } else {
            Err(errors)
        }
    }

    fn is_empty(&self) -> bool {
        self.taken_at.is_none()
            && self.gps.is_none()
            && self.artist.is_none()
            && self.copyright.is_none()
    }
}

impl Catalog {
    /// Apply `edit` to one photo. Invalid input comes back as per-field
    /// errors with nothing written; an empty list means it all landed. An
    /// unknown photo or a sidecar I/O failure is an `Err` (catalog untouched).
    pub fn edit_metadata(
        &mut self,
        file_id: i64,
        edit: &MetadataEdit,
    ) -> SyncResult<Vec<FieldError>> {
        let taken_at: Option<String> = self
            .conn()
            .query_row("SELECT taken_at FROM files WHERE id = ?1", [file_id], |r| r.get(0))
            .optional()?
            .ok_or(crate::SyncError::UnknownFile(file_id))?;
        if edit.is_empty() {
            return Ok(Vec::new());
        }

        // The offset a bare date inherits: the sidecar's, else the catalog's.
        let offset = if edit.taken_at.is_some() {
            current_offset(self, file_id, taken_at.as_deref())
        } else {
            None
        };
        let checked = match edit.check(offset) {
            Ok(checked) => checked,
            Err(errors) => return Ok(errors),
        };

        self.mutate_user_data(file_id, |sc| {
            if let Some(time) = &checked.taken_at {
                sc.set_capture_time(time);
            }
            match checked.gps {
                Some(Some((lat, lon))) => sc.set_gps(lat, lon),
                Some(None) => sc.clear_gps(),
                None => {}
            }
            if let Some(artist) = &edit.artist {
                match artist.trim() {
                    "" => sc.clear_artist(),
                    name => sc.set_artist(name),
                }
            }
            if let Some(notice) = &edit.copyright {
                match notice.trim() {
                    "" => sc.clear_copyright(),
                    text => sc.set_copyright(text),
                }
            }
        })?;
        Ok(Vec::new())
    }
}

fn current_offset(cat: &Catalog, file_id: i64, taken_at: Option<&str>) -> Option<i32> {
    let sidecar_path: Option<String> = cat
        .conn()
        .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))
        .ok()
        .map(|p: String| crate::sync::sidecar_path(&p).to_string_lossy().into_owned());
    let from_sidecar = sidecar_path
        .and_then(|p| fotobase_metadata::Sidecar::read(std::path::Path::new(&p)).ok())
        .and_then(|sc| sc.capture_time());
    from_sidecar.or_else(|| taken_at.and_then(CaptureTime::parse)).and_then(|t| t.offset_minutes())
}
