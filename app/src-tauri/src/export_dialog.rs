//! Export dialog (TASK-04-015, PLAN §2J File menu / §6 P2b Export).
//!
//! The dialog's options map onto the shared full-res export engine
//! (TASK-04-018): decode → recipe at full resolution → resize to the
//! horizontal/vertical limit box → watermark stamped onto the output
//! pixels → JPEG encode at the chosen quality. The watermark is not a
//! recipe op — previews must never show it. A batch of photos exports in
//! the order the frontend passes (current grid sort), each into its own
//! file; numbering and collision avoidance keep every output file distinct
//! and never overwrite an existing file.
//!
//! Like Save-as (04-017) this path never touches the originals, and like
//! every heavy job it runs in the bounded job queue (PLAN §3 rule 4).

use std::path::{Path, PathBuf};

use fotobase_catalog::Catalog;
use fotobase_render::{export_image, ExportFormat, ExportOptions};

/// The empty recipe: photos without edits export through the same engine
/// (decode → resize → encode) so resize/quality/watermark apply uniformly.
const NO_RECIPE: &str = r#"{"spec":"fotobase.recipe","version":1,"ops":[]}"#;

/// Dialog options for one export batch.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ExportRequest {
    /// Horizontal pixel limit (`None` = original size). Never upscales.
    pub resize_max_width: Option<u32>,
    /// Vertical pixel limit (`None` = original size). Never upscales.
    pub resize_max_height: Option<u32>,
    /// JPEG quality 0–100 (the dialog's preset table resolves to this).
    pub quality: u8,
    /// Add `-N` numbers to file names to preserve order (§2J).
    pub numbered: bool,
    /// Watermark text stamped onto the output pixels (`None`/empty = off).
    pub watermark: Option<String>,
}

/// One exported photo.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ExportedFile {
    pub file_id: i64,
    pub path: PathBuf,
}

/// A photo that failed mid-batch; the rest of the batch continues.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FailedExport {
    pub file_id: i64,
    pub error: String,
}

/// Batch result: everything that landed plus everything that failed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ExportBatch {
    pub exported: Vec<ExportedFile>,
    pub failed: Vec<FailedExport>,
}

/// Failure of the batch setup itself (destination unreadable, ids unknown).
#[derive(Debug, thiserror::Error)]
pub enum ExportDialogError {
    #[error("destination is not a directory: {0}")]
    Destination(String),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Sync(#[from] fotobase_catalog::SyncError),
}

/// The output file name for photo `index` of the batch. Numbered mode adds
/// the batch-order suffix (`stem-1.jpg`, `stem-2.jpg`, …) so the export
/// order survives even when file names repeat; collision avoidance bumps
/// the suffix past anything already in the destination *or* already claimed
/// earlier in the batch — an existing file is never overwritten and every
/// name in the batch is distinct.
fn output_name(
    filename: &str,
    index: usize,
    numbered: bool,
    dest: &Path,
    claimed: &mut std::collections::HashSet<String>,
) -> String {
    let stem = Path::new(filename).file_stem().and_then(|s| s.to_str()).unwrap_or("photo");
    let mut suffix = if numbered { index + 1 } else { 0 };
    loop {
        let name = if suffix == 0 { format!("{stem}.jpg") } else { format!("{stem}-{suffix}.jpg") };
        if !dest.join(&name).exists() && !claimed.contains(&name) {
            claimed.insert(name.clone());
            return name;
        }
        suffix += 1;
    }
}

/// Export the photos `ids` (in the caller's order — the current grid sort)
/// into `dest`, reporting progress `(done, total)` after each photo.
pub fn export_photos(
    cat: &Catalog,
    ids: &[i64],
    dest: &Path,
    req: &ExportRequest,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<ExportBatch, ExportDialogError> {
    if !dest.is_dir() {
        return Err(ExportDialogError::Destination(dest.to_string_lossy().into_owned()));
    }

    let total = ids.len();
    let mut claimed = std::collections::HashSet::new();
    let mut batch = ExportBatch { exported: Vec::new(), failed: Vec::new() };
    for (index, id) in ids.iter().enumerate() {
        let row: (String, Option<String>) =
            cat.conn().query_row("SELECT path, filename FROM files WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let (path, filename) = row;
        let filename = filename.unwrap_or_else(|| "photo.jpg".to_owned());
        let name = output_name(&filename, index, req.numbered, dest, &mut claimed);
        let out = dest.join(&name);
        let recipe = cat.file_recipe(*id)?.unwrap_or_else(|| NO_RECIPE.to_owned());
        let opts = ExportOptions {
            format: ExportFormat::Jpeg,
            quality: req.quality,
            resize_max_width: req.resize_max_width,
            resize_max_height: req.resize_max_height,
            watermark: req.watermark.clone(),
            ..ExportOptions::default()
        };
        match export_image(Path::new(&path), &out, &recipe, &opts) {
            Ok(_) => batch.exported.push(ExportedFile { file_id: *id, path: out }),
            Err(e) => batch.failed.push(FailedExport { file_id: *id, error: e.to_string() }),
        }
        progress(index + 1, total);
    }
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Numbered mode: batch order 1..N, every name distinct, nothing that
    // already exists in the destination is ever picked as a target.
    #[test]
    fn numbered_names_are_sequential_and_collision_free() {
        let dest = tempfile::tempdir().unwrap();
        // A pre-existing `a-1.jpg` must not be reused or overwritten.
        std::fs::write(dest.path().join("a-1.jpg"), b"old").unwrap();
        let mut claimed = std::collections::HashSet::new();
        let n1 = output_name("a.jpg", 0, true, dest.path(), &mut claimed);
        let n2 = output_name("a.jpg", 1, true, dest.path(), &mut claimed);
        let n3 = output_name("b.jpg", 2, true, dest.path(), &mut claimed);
        assert_eq!(n1, "a-2.jpg");
        assert_eq!(n2, "a-3.jpg");
        assert_eq!(n3, "b-3.jpg");
        let mut names = vec![n1, n2, n3];
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 3, "all names distinct");
    }

    // Unnumbered mode: plain names, auto-suffix only on collision.
    #[test]
    fn unnumbered_names_collide_onto_a_free_suffix() {
        let dest = tempfile::tempdir().unwrap();
        let mut claimed = std::collections::HashSet::new();
        assert_eq!(output_name("a.jpg", 0, false, dest.path(), &mut claimed), "a.jpg");
        std::fs::write(dest.path().join("a.jpg"), b"old").unwrap();
        assert_eq!(output_name("a.jpg", 0, false, dest.path(), &mut claimed), "a-1.jpg");
        std::fs::write(dest.path().join("a-1.jpg"), b"old").unwrap();
        assert_eq!(output_name("a.jpg", 0, false, dest.path(), &mut claimed), "a-2.jpg");
    }
}
