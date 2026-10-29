//! Date/time adjustment commands' logic (TASK-03-019, PLAN §6 P2a). The
//! catalog's sync engine owns the write-through (sidecar first, catalog
//! second); these helpers add the app-facing shapes: a disk-free preview and
//! the batch run that the job queue drives on its own catalog connection
//! with plain-counter progress events.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{AdjustReport, Catalog, TimeAdjustment, TimePreview};
use serde::Serialize;

/// Tauri event the batch reports progress on (§3: counters, no image bytes).
pub const DATETIME_EVENT: &str = "datetime-progress";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DateTimeProgress {
    pub done: usize,
    pub total: usize,
}

pub fn preview(
    cat: &Catalog,
    ids: &[i64],
    adjustment: &TimeAdjustment,
) -> Result<Vec<TimePreview>, String> {
    cat.preview_capture_times(ids, adjustment).map_err(|e| e.to_string())
}

/// One batch run against the catalog at `catalog_path`. Cancellation is a
/// plain flag the caller bridges from the queue's cancel token.
pub fn run(
    catalog_path: &Path,
    ids: &[i64],
    adjustment: &TimeAdjustment,
    cancel: &AtomicBool,
    emit: &dyn Fn(DateTimeProgress),
) -> Result<AdjustReport, String> {
    let mut cat = Catalog::open(catalog_path).map_err(|e| format!("open catalog: {e}"))?;
    cat.adjust_capture_times(ids, adjustment, cancel, &|done, total| {
        emit(DateTimeProgress { done, total });
    })
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fotobase_metadata::Sidecar;

    fn catalog_with_photo(dir: &Path, taken_at: &str) -> (i64, std::path::PathBuf) {
        let mut cat = Catalog::open(dir.join("catalog.db")).unwrap();
        let original = dir.join("a.jpg");
        std::fs::write(&original, b"bytes").unwrap();
        let folder = cat.seed_folder(dir.to_str().unwrap()).unwrap();
        cat.conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, taken_at, media_type)
                 VALUES (?1, ?2, 'a.jpg', 5, 0.0, ?3, 'image')",
                rusqlite::params![folder, original.to_str().unwrap(), taken_at],
            )
            .unwrap();
        (cat.conn().last_insert_rowid(), original)
    }

    #[test]
    fn run_writes_the_sidecar_and_streams_progress() {
        let dir = tempfile::tempdir().unwrap();
        let (id, original) = catalog_with_photo(dir.path(), "2024-06-01T12:00:00+02:00");
        let seen = std::cell::RefCell::new(Vec::new());

        let report = run(
            &dir.path().join("catalog.db"),
            &[id],
            &TimeAdjustment::Shift { seconds: -2 * 3600 },
            &AtomicBool::new(false),
            &|p| seen.borrow_mut().push(p),
        )
        .unwrap();

        assert_eq!(report.adjusted, 1);
        assert_eq!(*seen.borrow(), vec![DateTimeProgress { done: 1, total: 1 }]);
        let sc =
            Sidecar::read(&fotobase_catalog::sidecar_path(original.to_str().unwrap())).unwrap();
        assert_eq!(sc.capture_time().unwrap().to_xmp(), "2024-06-01T10:00:00+02:00");
    }

    #[test]
    fn preview_is_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let (id, original) = catalog_with_photo(dir.path(), "2024-06-01T12:00:00");
        let cat = Catalog::open(dir.path().join("catalog.db")).unwrap();

        let rows = preview(&cat, &[id], &TimeAdjustment::Shift { seconds: 60 }).unwrap();

        assert_eq!(rows[0].after.as_deref(), Some("2024-06-01T12:01:00"));
        assert!(!fotobase_catalog::sidecar_path(original.to_str().unwrap()).exists());
    }

    #[test]
    fn an_invalid_offset_is_refused_with_a_readable_error() {
        let dir = tempfile::tempdir().unwrap();
        let (id, _) = catalog_with_photo(dir.path(), "2024-06-01T12:00:00");
        let cat = Catalog::open(dir.path().join("catalog.db")).unwrap();

        let err = preview(&cat, &[id], &TimeAdjustment::RelabelOffset { offset_minutes: 5000 })
            .unwrap_err();

        assert!(err.contains("offset"), "{err}");
    }
}
