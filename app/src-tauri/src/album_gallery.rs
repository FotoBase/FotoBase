//! Album → static web gallery (TASK-10-01-01, PLAN §7 F1 layer 1).
//!
//! Resolves an album's photos from the catalog and hands them to the
//! standalone gallery generator in `fotobase-export` (TASK-08-016), which
//! renders them through the shared export engine into a self-contained site.
//! Only the album's photos are read: originals are never touched, and the
//! catalog is only queried. The output depends on nothing but the album's
//! content (photos, order, captions, recipes) and the request, so the same
//! album always yields byte-identical files. Uploading the folder is the
//! export plugins' job (TASK-10-01-02).

use std::path::Path;

use fotobase_catalog::Catalog;
use fotobase_export::gallery::{
    builtin_template, generate, GalleryError, GalleryOptions, GalleryPhoto, GalleryReport,
    RenderPipeline,
};
use rusqlite::OptionalExtension;

/// Options for one album publish.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AlbumGalleryRequest {
    /// Built-in template id (`grid`, `dark`).
    pub template: String,
    /// Page title; the album name when `None`.
    pub title: Option<String>,
    /// Longest edge of the viewer images.
    pub image_max_edge: u32,
    /// Longest edge of the grid thumbnails.
    pub thumb_max_edge: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum AlbumGalleryError {
    #[error("unknown album {0}")]
    UnknownAlbum(i64),
    #[error("album has no photos to publish")]
    EmptyAlbum,
    #[error("unknown gallery template {0:?}")]
    UnknownTemplate(String),
    #[error(transparent)]
    Gallery(#[from] GalleryError),
    #[error(transparent)]
    Catalog(#[from] fotobase_catalog::CatalogError),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Sync(#[from] fotobase_catalog::SyncError),
}

/// What a publish did, for the UI.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AlbumGalleryOutput {
    pub photos: usize,
    pub written: usize,
    pub unchanged: usize,
    pub removed: usize,
    /// The site's entry page.
    pub index: std::path::PathBuf,
}

impl AlbumGalleryOutput {
    fn new(report: &GalleryReport, dest: &Path) -> Self {
        Self {
            photos: report.photos,
            written: report.written,
            unchanged: report.unchanged,
            removed: report.removed,
            index: dest.join("index.html"),
        }
    }
}

/// Writes album `album_id` as a static site into `dest` (created when
/// missing). Re-running rewrites only what changed.
pub fn export_album_gallery(
    cat: &Catalog,
    album_id: i64,
    dest: &Path,
    req: &AlbumGalleryRequest,
) -> Result<AlbumGalleryOutput, AlbumGalleryError> {
    let album = cat.album(album_id)?.ok_or(AlbumGalleryError::UnknownAlbum(album_id))?;
    let template = builtin_template(&req.template)
        .ok_or_else(|| AlbumGalleryError::UnknownTemplate(req.template.clone()))?;

    let mut photos = Vec::new();
    for file in cat.list_album_files(album_id, usize::MAX)? {
        if file.media_type != "image" {
            continue;
        }
        let path: Option<String> = cat
            .conn()
            .query_row("SELECT path FROM files WHERE id = ?1", [file.id], |r| r.get(0))
            .optional()?;
        let Some(path) = path else { continue };
        photos.push(GalleryPhoto {
            source: path.into(),
            title: None,
            caption: file.caption.filter(|c| !c.trim().is_empty()),
            recipe: cat.file_recipe(file.id)?,
        });
    }
    if photos.is_empty() {
        return Err(AlbumGalleryError::EmptyAlbum);
    }

    let options = GalleryOptions {
        title: req.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or(album.name),
        description: album.description,
        template: template.id.clone(),
        image_max_edge: req.image_max_edge,
        thumb_max_edge: req.thumb_max_edge,
        ..GalleryOptions::default()
    };
    let report = generate(dest, &photos, &options, &template, &RenderPipeline)?;
    Ok(AlbumGalleryOutput::new(&report, dest))
}
