//! Share photos to the platform share sheet (TASK-09-005, PLAN §6 Phase 5
//! "share"; iOS and Android).
//!
//! The selected photos are rendered into a scratch folder by the same export
//! path the Export and Email dialogs use (edits baked in, never upscaled,
//! longest edge capped at [`SHARE_MAX_EDGE`] so a RAW-sized selection does not
//! exhaust phone memory), then every file goes to the native share sheet in a
//! single call. Originals are never opened for writing, and the sheet receives
//! copies — never the library files.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use fotobase_catalog::Catalog;

use crate::email_share::{new_scratch_dir, sweep_stale, STALE_AFTER};
use crate::export_dialog::{
    export_photos, ExportDialogError, ExportRequest, ExportedFile, FailedExport,
};

/// Longest edge of a shared photo; smaller photos keep their size.
pub const SHARE_MAX_EDGE: u32 = 4096;
/// JPEG quality of shared copies.
pub const SHARE_QUALITY: u8 = 90;

/// Rendered copies waiting in the scratch folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedShare {
    pub dir: PathBuf,
    pub exported: Vec<ExportedFile>,
    /// Photos that could not be rendered; the rest of the batch goes on.
    pub failed: Vec<FailedExport>,
}

impl PreparedShare {
    /// The rendered files, in the caller's photo order.
    pub fn files(&self) -> Vec<PathBuf> {
        self.exported.iter().map(|f| f.path.clone()).collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ShareError {
    #[error("could not create the scratch folder: {0}")]
    Scratch(#[from] std::io::Error),
    #[error(transparent)]
    Export(#[from] ExportDialogError),
    #[error("no photo could be prepared for sharing")]
    NothingExported,
    #[error("{0}")]
    Sheet(String),
}

/// The platform share API behind [`hand_off`].
pub trait ShareTarget {
    /// Open the share sheet with all of `files` attached.
    fn share_files(&self, files: &[PathBuf]) -> Result<(), String>;
}

/// Render the photos `ids` (in the caller's order) into a fresh folder under
/// `root`, reporting progress `(done, total)` after each photo.
pub fn prepare_share(
    cat: &Catalog,
    ids: &[i64],
    root: &Path,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<PreparedShare, ShareError> {
    sweep_stale(root, SystemTime::now(), STALE_AFTER);
    let dir = new_scratch_dir(root, "share")?;
    let export = ExportRequest {
        resize_max_width: Some(SHARE_MAX_EDGE),
        resize_max_height: Some(SHARE_MAX_EDGE),
        quality: SHARE_QUALITY,
        numbered: false,
        watermark: None,
    };
    let batch = match export_photos(cat, ids, &dir, &export, progress) {
        Ok(batch) => batch,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e.into());
        }
    };
    if batch.exported.is_empty() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(ShareError::NothingExported);
    }
    Ok(PreparedShare { dir, exported: batch.exported, failed: batch.failed })
}

/// Hand all prepared files to the share sheet in one invocation.
pub fn hand_off(prepared: &PreparedShare, target: &dyn ShareTarget) -> Result<(), ShareError> {
    target.share_files(&prepared.files()).map_err(ShareError::Sheet)
}

/// The native share sheet through the `share-sheet` plugin.
pub struct NativeShareSheet(pub tauri::AppHandle);

impl ShareTarget for NativeShareSheet {
    fn share_files(&self, files: &[PathBuf]) -> Result<(), String> {
        use tauri_plugin_share_sheet::ShareSheetExt as _;
        self.0.share_sheet().share_files(files).map_err(|e| e.to_string())
    }
}
