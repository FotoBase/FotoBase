//! Date/time adjustment (TASK-03-019, PLAN §6 P2a): shift capture time, fix
//! a wrong timezone, or align a second camera's clock. The adjusted time is
//! user data, so it is written to the sidecar's `exif:DateTimeOriginal`
//! through the sync engine's write-through (sidecar first, catalog second,
//! 00-010 §3); `files.taken_at` is only the derived cache of it.
//!
//! The starting point for a photo is its sidecar's capture time, else the
//! catalog's `taken_at` (the scan-time EXIF value), else its `mtime` — the
//! same fallback the date filter and folder years already use. Originals are
//! never touched.

use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use fotobase_metadata::CaptureTime;

use crate::sync::{SyncError, SyncResult};
use crate::Catalog;

/// What to do to the capture times of a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimeAdjustment {
    /// Move the wall clock by `seconds` (negative = earlier); the offset
    /// stays. Also the multi-camera merge: the dialog turns "this moment
    /// reads X on the right camera and Y on this one" into `X − Y`.
    Shift { seconds: i64 },
    /// The clock reading is right but its zone label is wrong (camera tagged
    /// UTC while showing local time): relabel, keep the reading.
    RelabelOffset { offset_minutes: i32 },
    /// The moment is right but shown in the wrong zone (camera left on UTC):
    /// show the same instant in this offset. Naive readings count as UTC.
    ConvertOffset { offset_minutes: i32 },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AdjustError {
    #[error("capture time {0:?} is not a readable date and time")]
    Unreadable(String),
    #[error("the adjusted time falls outside years 1–9999")]
    OutOfRange,
    #[error("UTC offset of {0} minutes is outside −12:00…+14:00")]
    InvalidOffset(i32),
}

impl TimeAdjustment {
    /// Validate the parameters without a photo (offset range).
    pub fn validate(&self) -> Result<(), AdjustError> {
        match *self {
            TimeAdjustment::RelabelOffset { offset_minutes }
            | TimeAdjustment::ConvertOffset { offset_minutes }
                if !(-12 * 60..=14 * 60).contains(&offset_minutes) =>
            {
                Err(AdjustError::InvalidOffset(offset_minutes))
            }
            _ => Ok(()),
        }
    }

    pub fn apply(&self, time: &CaptureTime) -> Result<CaptureTime, AdjustError> {
        self.validate()?;
        match *self {
            TimeAdjustment::Shift { seconds } => time.shifted(seconds),
            TimeAdjustment::RelabelOffset { offset_minutes } => {
                time.with_offset_keeping_wall_clock(offset_minutes)
            }
            TimeAdjustment::ConvertOffset { offset_minutes } => {
                time.converted_to_offset(offset_minutes)
            }
        }
        .ok_or(AdjustError::OutOfRange)
    }
}

/// One photo's row in the preview dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TimePreview {
    pub id: i64,
    pub before: Option<String>,
    pub after: Option<String>,
    /// Why this photo would be skipped; `after` is `None` then.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdjustFailure {
    pub id: i64,
    pub error: String,
}

/// Outcome of one batch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AdjustReport {
    pub adjusted: usize,
    pub failed: Vec<AdjustFailure>,
    pub cancelled: bool,
}

/// Capture time to start from. A blank `taken_at` falls back to the mtime; a
/// non-blank one that is not a date is an error rather than a guess.
fn base_time(
    sidecar: Option<CaptureTime>,
    taken_at: Option<&str>,
    mtime: f64,
) -> Result<CaptureTime, AdjustError> {
    if let Some(time) = sidecar {
        return Ok(time);
    }
    match taken_at.map(str::trim).filter(|t| !t.is_empty()) {
        Some(text) => {
            CaptureTime::parse(text).ok_or_else(|| AdjustError::Unreadable(text.to_string()))
        }
        None => CaptureTime::from_unix(mtime.floor() as i64)
            .ok_or_else(|| AdjustError::Unreadable(format!("mtime {mtime}"))),
    }
}

impl Catalog {
    /// `(taken_at, mtime)` of a catalog row.
    fn time_inputs(&self, file_id: i64) -> SyncResult<(Option<String>, f64)> {
        self.conn()
            .query_row("SELECT taken_at, mtime FROM files WHERE id = ?1", [file_id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?
            .ok_or(SyncError::UnknownFile(file_id))
    }

    /// The capture time the catalog currently knows for a photo.
    fn cached_capture_time(&self, file_id: i64) -> SyncResult<CaptureTime> {
        let (taken_at, mtime) = self.time_inputs(file_id)?;
        Ok(base_time(None, taken_at.as_deref(), mtime)?)
    }

    /// What `adjustment` would do to each photo, from catalog data only — no
    /// sidecar is read or written. Photos that cannot be adjusted carry the
    /// reason in `error`.
    pub fn preview_capture_times(
        &self,
        ids: &[i64],
        adjustment: &TimeAdjustment,
    ) -> SyncResult<Vec<TimePreview>> {
        adjustment.validate()?;
        Ok(ids
            .iter()
            .map(|&id| {
                let outcome = self.cached_capture_time(id).and_then(|before| {
                    let after = adjustment.apply(&before)?;
                    Ok((before, after))
                });
                match outcome {
                    Ok((before, after)) => TimePreview {
                        id,
                        before: Some(before.to_xmp()),
                        after: Some(after.to_xmp()),
                        error: None,
                    },
                    Err(err) => TimePreview {
                        id,
                        before: self.cached_capture_time(id).ok().map(|t| t.to_xmp()),
                        after: None,
                        error: Some(err.to_string()),
                    },
                }
            })
            .collect())
    }

    /// Apply `adjustment` to every photo, one sidecar write each (00-010 §3
    /// coalescing). A photo that cannot be adjusted — unknown id, unreadable
    /// date, result out of range, sidecar I/O error — is reported in
    /// `failed` and the rest carry on; its sidecar and catalog row are left
    /// as they were. `progress(done, total)` fires after every attempted
    /// photo; setting `cancel` stops before the next one.
    pub fn adjust_capture_times(
        &mut self,
        ids: &[i64],
        adjustment: &TimeAdjustment,
        cancel: &AtomicBool,
        progress: &dyn Fn(usize, usize),
    ) -> SyncResult<AdjustReport> {
        adjustment.validate()?;
        let mut report = AdjustReport::default();
        let total = ids.len();
        for (index, &id) in ids.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                report.cancelled = true;
                break;
            }
            match self.adjust_one(id, adjustment) {
                Ok(()) => report.adjusted += 1,
                Err(err) => report.failed.push(AdjustFailure { id, error: err.to_string() }),
            }
            progress(index + 1, total);
        }
        Ok(report)
    }

    fn adjust_one(&mut self, file_id: i64, adjustment: &TimeAdjustment) -> SyncResult<()> {
        let (taken_at, mtime) = self.time_inputs(file_id)?;
        self.try_mutate_user_data(file_id, |sc| {
            let base = base_time(sc.capture_time(), taken_at.as_deref(), mtime)?;
            sc.set_capture_time(&adjustment.apply(&base)?);
            Ok(())
        })
    }
}
