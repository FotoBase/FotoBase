//! Import from folder / removable drive (TASK-01-017, PLAN §2A/§6 P1).
//!
//! The plain-Rust half of the `start_import` command: it owns a catalog
//! connection (WAL — UI reads never block behind the import), runs the
//! scanner's import engine, and streams progress to the UI as plain
//! counters on the `import-progress` event. No image bytes ever cross IPC.
//!
//! Cancellation: the command holds a [`fotobase_jobs::CancelToken`] in
//! shared state; `cancel_import` trips it and the engine observes the flag
//! before every file and per chunk inside a copy — a cancelled import never
//! leaves a half-copied file at the destination (per-file cleanup).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use fotobase_catalog::Catalog;
use fotobase_scanner::{import_folder, ImportOptions, Naming};
use serde::Serialize;

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-import";

/// Tauri event the import reports progress on (§3: progress events, no
/// image bytes — this payload is plain counters).
pub const IMPORT_EVENT: &str = "import-progress";

/// Incremental progress for the UI, mirrored from the scanner engine.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportProgress {
    pub files_done: usize,
    pub files_total: usize,
    pub current_file: String,
}

/// Aggregated outcome of one import run, as the UI displays it.
#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct ImportReport {
    pub copied: usize,
    pub duplicates: usize,
    pub failed: usize,
    pub cancelled: bool,
}

/// Parse the `naming` string a Tauri command receives, with the user-typed
/// custom pattern.
pub fn parse_naming(naming: &str, custom_pattern: Option<String>) -> Option<Naming> {
    match naming {
        "date" => Some(Naming::DateFolders),
        "custom" => custom_pattern.map(Naming::Custom),
        _ => None,
    }
}

/// Run one import against the catalog at `catalog_path`, emitting progress.
/// Cancellation is a plain flag here; the command bridges its cancel token
/// onto it.
pub fn run_import(
    catalog_path: &Path,
    source: &Path,
    dest_root: &Path,
    options: &ImportOptions,
    cancel: &AtomicBool,
    emit: &dyn Fn(ImportProgress),
) -> Result<ImportReport, String> {
    let mut cat = Catalog::open(catalog_path).map_err(|e| format!("open catalog: {e}"))?;
    let report = import_folder(&mut cat, source, dest_root, options, cancel, &|p| {
        emit(ImportProgress {
            files_done: p.files_done,
            files_total: p.files_total,
            current_file: p.current_file.clone(),
        });
    })
    .map_err(|e| format!("import: {e}"))?;
    Ok(ImportReport {
        copied: report.copied,
        duplicates: report.duplicates,
        failed: report.failed,
        cancelled: report.cancelled,
    })
}

/// The source/destination pair a command validates before enqueueing.
#[derive(Clone, Debug)]
pub struct ImportRequest {
    pub source: PathBuf,
    pub dest_root: PathBuf,
    pub options: ImportOptions,
}

/// Validate a command's raw arguments. `exclude_duplicates` defaults to on
/// (PLAN §2: "Detect duplicates while importing", default-enabled).
pub fn parse_request(
    source: &str,
    destination: &str,
    naming: &str,
    custom_pattern: Option<String>,
    exclude_duplicates: Option<bool>,
) -> Option<ImportRequest> {
    if source.is_empty() || destination.is_empty() {
        return None;
    }
    let naming = parse_naming(naming, custom_pattern)?;
    Some(ImportRequest {
        source: PathBuf::from(source),
        dest_root: PathBuf::from(destination),
        options: ImportOptions { exclude_duplicates: exclude_duplicates.unwrap_or(true), naming },
    })
}

/// Check-and-set helper for the command's cancel bridging: flips `flag`
/// when the token (or the queue's own context) says stop.
pub fn sync_cancel(flag: &AtomicBool, cancelled: bool) {
    if cancelled {
        flag.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naming_parses_strictly() {
        assert_eq!(parse_naming("date", None), Some(Naming::DateFolders));
        assert_eq!(parse_naming("custom", None), None, "custom needs a pattern");
        assert_eq!(
            parse_naming("custom", Some("trip".into())),
            Some(Naming::Custom("trip".into()))
        );
        assert_eq!(parse_naming("Date", None), None);
        assert_eq!(parse_naming("", None), None);
    }

    #[test]
    fn request_defaults_exclude_duplicates_on() {
        let req = parse_request("/src", "/dst", "date", None, None).unwrap();
        assert!(req.options.exclude_duplicates, "the checkbox default is on");
        assert_eq!(req.options.naming, Naming::DateFolders);
        assert!(parse_request("", "/dst", "date", None, None).is_none());
        assert!(parse_request("/src", "", "date", None, None).is_none());
        assert!(parse_request("/src", "/dst", "bogus", None, None).is_none());
    }

    #[test]
    fn explicit_off_is_honored() {
        let req = parse_request("/src", "/dst", "custom", Some("in".into()), Some(false)).unwrap();
        assert!(!req.options.exclude_duplicates);
    }
}
