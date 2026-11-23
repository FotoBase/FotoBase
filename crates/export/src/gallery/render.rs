//! Image rendering seam of the gallery generator (TASK-08-016).

use std::path::Path;

use fotobase_render::{export_image, ExportFormat, ExportOptions};

use super::GalleryError;

/// Recipe that leaves the pixels untouched.
pub const EMPTY_RECIPE: &str = r#"{"spec":"fotobase.recipe","version":1,"ops":[]}"#;

pub struct RenderRequest<'a> {
    pub source: &'a Path,
    pub recipe: &'a str,
    /// Longest edge of the output; sources are never upscaled.
    pub max_edge: u32,
    pub quality: u8,
}

pub struct RenderedImage {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Produces one resized JPEG. Blocking.
pub trait ImageRenderer {
    fn render(&self, request: &RenderRequest<'_>) -> Result<RenderedImage, GalleryError>;
}

/// The shared export engine (`fotobase-render`): decode, recipe, color
/// management, resize, JPEG encode — the same pipeline as Save and Upload
/// (PLAN §3 rule 5).
pub struct RenderPipeline;

impl ImageRenderer for RenderPipeline {
    fn render(&self, request: &RenderRequest<'_>) -> Result<RenderedImage, GalleryError> {
        let fail =
            |message: String| GalleryError::Render { path: request.source.to_path_buf(), message };
        let scratch = tempfile::tempdir().map_err(GalleryError::Io)?;
        let output = scratch.path().join("out.jpg");
        let options = ExportOptions {
            format: ExportFormat::Jpeg,
            quality: request.quality,
            resize_max_edge: Some(request.max_edge),
            ..ExportOptions::default()
        };
        let result = export_image(request.source, &output, request.recipe, &options)
            .map_err(|e| fail(e.to_string()))?;
        let jpeg = std::fs::read(&output).map_err(GalleryError::Io)?;
        Ok(RenderedImage { jpeg, width: result.width, height: result.height })
    }
}
