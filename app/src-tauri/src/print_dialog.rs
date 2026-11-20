//! Print dialog backend (TASK-08-004, PLAN §2J Print, §6 P4).
//!
//! Two entry points over the render crate's layout engine
//! (`fotobase_render::print`):
//! - [`print_plan`] resolves the selection against the catalog (edited
//!   aspect, copies, text line) and returns the page plan as JSON — cell
//!   rectangles, fit rectangles, text bands — which the dialog's live paper
//!   preview positions its `preview://` images from. No pixels cross IPC.
//! - [`print_pages`] rasterizes the same plan at print resolution through
//!   the shared recipe engine and writes one PNG per sheet.
//! - [`export_pdf`] and [`print_to_printer`] (TASK-08-005) are the other two
//!   sinks of that same rendering path: a PDF with one page per sheet, and a
//!   job handed to the OS spooler ([`crate::printers`]). Same plan, same
//!   rasters — the PDF cannot differ from the physical print.
//!
//! Like every heavy job, [`print_pages`] runs in the bounded job queue (the
//! command wraps it); originals are only ever read.

use std::path::{Path, PathBuf};

use fotobase_catalog::{Catalog, PhotoInfo};
use fotobase_render::print::{
    edited_dimensions, plan_pages, write_pages, write_pdf, PagePlan, PrintFailure, PrintItem,
    PrintOptions, PrintPhotoSource, TextSource,
};

use crate::printers::{Document, DocumentKind, SpoolError, Spooler};

/// Print resolution of the written page rasters.
pub const PRINT_DPI: u32 = 300;

/// Job title shown in the OS print queue.
const PRINT_TITLE: &str = "Fotobase";

/// Highest copies-per-photo the dialog accepts.
pub const MAX_COPIES: u32 = 99;

/// Dialog state for one print job.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PrintRequest {
    /// Copies of every selected photo (a 4×6 sheet with 2 copies of each).
    pub copies: u32,
    pub options: PrintOptions,
}

/// A selected photo the preview needs an image for.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlanPhoto {
    pub file_id: i64,
    pub filename: String,
    /// Edit recipe JSON; the dialog hands it to `render_edit_preview`.
    pub recipe: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PrintPlanReply {
    pub pages: Vec<PagePlan>,
    pub photos: Vec<PlanPhoto>,
}

/// What a print job produced: the files that landed (page PNGs, or the one
/// PDF), the sheet count, the spooler's job id for a physical print, and the
/// photos that could not be placed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PrintBatch {
    pub files: Vec<PathBuf>,
    pub pages: usize,
    pub job_id: Option<String>,
    pub failed: Vec<PrintFailure>,
}

#[derive(Debug, thiserror::Error)]
pub enum PrintDialogError {
    #[error("no photos to print")]
    Empty,
    #[error("unknown photo {0}")]
    UnknownPhoto(i64),
    #[error("destination is not a directory: {0}")]
    Destination(String),
    #[error("the PDF's folder does not exist: {0}")]
    PdfFolder(String),
    #[error("print cancelled")]
    Cancelled,
    #[error("print temp files: {0}")]
    Temp(#[from] std::io::Error),
    #[error(transparent)]
    Spool(#[from] SpoolError),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Sync(#[from] fotobase_catalog::SyncError),
    #[error(transparent)]
    Catalog(#[from] fotobase_catalog::CatalogError),
    #[error(transparent)]
    Print(#[from] fotobase_render::print::PrintError),
}

/// One-line EXIF summary: date, camera, exposure, aperture, ISO, focal length.
fn exif_line(info: &PhotoInfo) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(d) = info.taken_at.as_deref().filter(|d| !d.is_empty()) {
        parts.push(d.split('T').next().unwrap_or(d).to_owned());
    }
    if let Some(c) = info.camera.as_deref().filter(|c| !c.is_empty()) {
        parts.push(c.to_owned());
    }
    let mut shot: Vec<String> = Vec::new();
    if let Some(e) = info.exposure.as_deref().filter(|e| !e.is_empty()) {
        shot.push(format!("{e}s"));
    }
    if let Some(f) = info.f_number {
        shot.push(format!("f/{f}"));
    }
    if let Some(iso) = info.iso {
        shot.push(format!("ISO {iso}"));
    }
    if let Some(mm) = info.focal_length {
        shot.push(format!("{mm}mm"));
    }
    if !shot.is_empty() {
        parts.push(shot.join(" "));
    }
    parts.join(" · ")
}

/// The text printed with a photo for the chosen source.
fn label_for(source: TextSource, info: &PhotoInfo, caption: Option<String>) -> Option<String> {
    let text = match source {
        TextSource::None => return None,
        TextSource::Caption => caption.unwrap_or_default(),
        TextSource::Filename => info.filename.clone(),
        TextSource::Exif => exif_line(info),
    };
    (!text.trim().is_empty()).then_some(text)
}

/// The path-and-recipe inputs of a print job: what the layout engine needs
/// about the selection, in selection order (the caller passes the current
/// grid sort), plus the preview sources.
pub struct PreparedPrint {
    items: Vec<PrintItem>,
    sources: Vec<PrintPhotoSource>,
    photos: Vec<PlanPhoto>,
    options: PrintOptions,
}

/// Resolve the selection against the catalog.
fn load_job(
    cat: &Catalog,
    ids: &[i64],
    req: &PrintRequest,
) -> Result<PreparedPrint, PrintDialogError> {
    if ids.is_empty() {
        return Err(PrintDialogError::Empty);
    }
    let mut items = Vec::new();
    let mut sources = Vec::new();
    let mut photos = Vec::new();
    for id in ids {
        let info = cat.photo_info(*id)?.ok_or(PrintDialogError::UnknownPhoto(*id))?;
        let recipe = cat.file_recipe(*id)?.unwrap_or_default();
        let caption = cat.file_caption(*id)?;
        // Edited aspect, so a cropped photo is laid out as it will print.
        let (width, height) = edited_dimensions(
            u32::try_from(info.width.unwrap_or(3)).unwrap_or(3),
            u32::try_from(info.height.unwrap_or(2)).unwrap_or(2),
            &recipe,
        );
        items.push(PrintItem {
            file_id: *id,
            width,
            height,
            copies: req.copies.clamp(1, MAX_COPIES),
            label: label_for(req.options.text.source, &info, caption),
        });
        sources.push(PrintPhotoSource {
            file_id: *id,
            path: PathBuf::from(&info.path),
            recipe_json: recipe.clone(),
        });
        photos.push(PlanPhoto { file_id: *id, filename: info.filename, recipe });
    }
    Ok(PreparedPrint { items, sources, photos, options: req.options.clone() })
}

/// The page plan for the selection — what the live preview draws.
pub fn print_plan(
    cat: &Catalog,
    ids: &[i64],
    req: &PrintRequest,
) -> Result<PrintPlanReply, PrintDialogError> {
    let job = load_job(cat, ids, req)?;
    Ok(PrintPlanReply { pages: plan_pages(&req.options, &job.items), photos: job.photos })
}

/// Resolve a print job while the catalog is open, so the (long)
/// rasterization can run without it.
pub fn prepare_print(
    cat: &Catalog,
    ids: &[i64],
    req: &PrintRequest,
) -> Result<PreparedPrint, PrintDialogError> {
    load_job(cat, ids, req)
}

/// Rasterize the job into `dest`, reporting the finished page count.
pub fn print_pages(
    job: &PreparedPrint,
    dest: &Path,
    progress: &mut dyn FnMut(usize),
) -> Result<PrintBatch, PrintDialogError> {
    if !dest.is_dir() {
        return Err(PrintDialogError::Destination(dest.to_string_lossy().into_owned()));
    }
    let (files, failed) =
        write_pages(&job.options, &job.items, &job.sources, PRINT_DPI, dest, progress)?;
    Ok(PrintBatch { pages: files.len(), files, job_id: None, failed })
}

/// Write the job as one PDF at `dest`, replaced atomically; a cancelled job
/// leaves an existing file alone. The folder must exist; the file gets a
/// `.pdf` extension when it has none.
pub fn export_pdf(
    job: &PreparedPrint,
    dest: &Path,
    progress: &mut dyn FnMut(usize),
    cancelled: &dyn Fn() -> bool,
) -> Result<PrintBatch, PrintDialogError> {
    let dest =
        if dest.extension().is_some() { dest.to_path_buf() } else { dest.with_extension("pdf") };
    if dest.parent().is_some_and(|p| !p.as_os_str().is_empty() && !p.is_dir()) {
        return Err(PrintDialogError::PdfFolder(dest.to_string_lossy().into_owned()));
    }
    let scratch = tempfile::tempdir()?;
    let rendered = scratch.path().join("fotobase-print.pdf");
    let mut pages = 0;
    let failed =
        write_pdf(&job.options, &job.items, &job.sources, PRINT_DPI, &rendered, &mut |n| {
            pages = n;
            progress(n);
        })?;
    if cancelled() {
        return Err(PrintDialogError::Cancelled);
    }
    let tmp = dest.with_extension("pdf.tmp");
    std::fs::copy(&rendered, &tmp)?;
    if let Err(e) = std::fs::rename(&tmp, &dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(PrintBatch { files: vec![dest], pages, job_id: None, failed })
}

/// Render the job into a scratch folder in the form the spooler takes and
/// submit it to `printer`. `cancelled` is checked once the sheets are
/// rendered, so a cancelled job never reaches the printer. The scratch
/// folder is removed afterwards; the spooler holds its own copy.
pub fn print_to_printer(
    job: &PreparedPrint,
    spooler: &dyn Spooler,
    printer: &str,
    progress: &mut dyn FnMut(usize),
    cancelled: &dyn Fn() -> bool,
) -> Result<PrintBatch, PrintDialogError> {
    let scratch = tempfile::tempdir()?;
    let (document, pages, failed) = match spooler.kind() {
        DocumentKind::Pdf => {
            let file = scratch.path().join("fotobase-print.pdf");
            let mut pages = 0;
            let failed =
                write_pdf(&job.options, &job.items, &job.sources, PRINT_DPI, &file, &mut |n| {
                    pages = n;
                    progress(n);
                })?;
            (vec![file], pages, failed)
        }
        DocumentKind::Pages => {
            let (files, failed) = write_pages(
                &job.options,
                &job.items,
                &job.sources,
                PRINT_DPI,
                scratch.path(),
                progress,
            )?;
            let pages = files.len();
            (files, pages, failed)
        }
    };
    if cancelled() {
        return Err(PrintDialogError::Cancelled);
    }
    let submission = match spooler.kind() {
        DocumentKind::Pdf => {
            spooler.submit(printer, job.options.paper, PRINT_TITLE, Document::Pdf(&document[0]))?
        }
        DocumentKind::Pages => {
            spooler.submit(printer, job.options.paper, PRINT_TITLE, Document::Pages(&document))?
        }
    };
    Ok(PrintBatch { files: Vec::new(), pages, job_id: submission.job_id, failed })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> PhotoInfo {
        PhotoInfo {
            id: 1,
            path: "/p/IMG_1.jpg".into(),
            filename: "IMG_1.jpg".into(),
            size: 1,
            mtime: 0.0,
            width: Some(3000),
            height: Some(2000),
            media_type: "image".into(),
            camera: Some("Canon EOS R".into()),
            lens: None,
            exposure: Some("1/250".into()),
            f_number: Some(2.8),
            iso: Some(100),
            focal_length: Some(35.0),
            taken_at: Some("2024-05-01T10:00:00".into()),
            latitude: None,
            longitude: None,
            artist: None,
            copyright: None,
        }
    }

    #[test]
    fn labels_follow_the_text_source() {
        let i = info();
        assert_eq!(label_for(TextSource::None, &i, Some("x".into())), None);
        assert_eq!(
            label_for(TextSource::Caption, &i, Some("Beach".into())).as_deref(),
            Some("Beach")
        );
        assert_eq!(label_for(TextSource::Caption, &i, None), None, "no caption, no band");
        assert_eq!(label_for(TextSource::Filename, &i, None).as_deref(), Some("IMG_1.jpg"));
        assert_eq!(
            label_for(TextSource::Exif, &i, None).as_deref(),
            Some("2024-05-01 · Canon EOS R · 1/250s f/2.8 ISO 100 35mm")
        );
    }
}
