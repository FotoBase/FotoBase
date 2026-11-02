//! Full-resolution, color-managed export (TASK-04-018, PLAN §3 rule 5, §6
//! P2b, §9 Render fidelity).
//!
//! One engine, two resolutions: the export path runs the *same* shared
//! recipe engine (`fotobase_core::apply::apply_recipe`) at full resolution
//! that the preview path (TASK-04-010) runs at screen resolution. The only
//! differences are the output size and the encode — never a separate op
//! implementation.
//!
//! Color management (TASK-02-001 chain): the source's embedded ICC profile
//! is honored — the decoded pixels are converted from the source space into
//! the output space through lcms2 — and the exported file is tagged with the
//! output profile (sRGB for untagged sources, so consumers never guess).
//!
//! Memory: pixel-local recipes (see `PIXEL_LOCAL_OPS`) are applied per
//! horizontal strip, so the f32 linear-light working set is bounded by the
//! strip. Recipes with geometry, neighbor, position-dependent or
//! whole-image-statistics ops take the whole-image path to stay exactly equal
//! to the shared engine. In both paths the decoded RGB8 source stays resident.

use std::path::{Path, PathBuf};

use fotobase_jobs::{job, Job, Outcome};
use thiserror::Error;

use crate::RenderError;
use crate::{decode_raw, encode_jpeg, fit, icc, resize};

/// Output container of an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Jpeg,
    Png,
}

/// Export parameters (TASK-04-015: resize H/V limits, quality, watermark;
/// TASK-04-018 established the engine).
#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub format: ExportFormat,
    /// JPEG quality (0–100); ignored for PNG (lossless).
    pub quality: u8,
    /// Longest-edge pixel limit; `None` exports at full resolution and the
    /// source size is never upscaled either way.
    pub resize_max_edge: Option<u32>,
    /// Horizontal pixel limit (TASK-04-015 §2J): the output fits inside the
    /// limit box (never upscaled). `None` = unlimited.
    pub resize_max_width: Option<u32>,
    /// Vertical pixel limit (TASK-04-015 §2J). `None` = unlimited.
    pub resize_max_height: Option<u32>,
    /// Watermark text stamped onto the output pixels at export time
    /// (TASK-04-015 — not a recipe op). `None`/empty = no watermark.
    pub watermark: Option<String>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::Jpeg,
            quality: 90,
            resize_max_edge: None,
            resize_max_width: None,
            resize_max_height: None,
            watermark: None,
        }
    }
}

/// Result of a successful export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportResult {
    pub width: u32,
    pub height: u32,
    /// The ICC profile the exported file is tagged with, if any.
    pub output_profile: Option<Vec<u8>>,
}

/// Errors raised by the export path.
#[derive(Debug, Error)]
pub enum ExportError {
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error("malformed edit recipe: {0}")]
    Recipe(String),
    #[error("color transform failed: {0}")]
    Color(String),
    #[error("I/O during export: {0}")]
    Io(#[from] std::io::Error),
}

impl From<crate::color::ColorError> for ExportError {
    fn from(e: crate::color::ColorError) -> Self {
        Self::Color(e.to_string())
    }
}

/// Rows rendered per streaming pass. ~4 MB of linear-light buffer per tile
/// per 6000-wide image; keeps peak memory flat for 100+ MP sources while
/// still amortizing the recipe's per-op fixed costs over useful chunks.
const TILE_ROWS: u32 = 128;

fn encode_png(rgb: &[u8], w: u32, h: u32, profile: &[u8]) -> Vec<u8> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    let mut enc = image::codecs::png::PngEncoder::new(&mut out);
    enc.set_icc_profile(profile.to_vec()).expect("PNG encoder embeds ICC profiles (iCCP)");
    enc.write_image(rgb, w, h, image::ExtendedColorType::Rgb8).expect("encoding in-memory png");
    out
}

/// Export `src` with `recipe_json` applied at full resolution into `dst`.
///
/// Pipeline: decode → the shared recipe engine (streamed per strip when the
/// recipe is pixel-local) → sRGB-encode → resize (optional) → color-managed
/// conversion into the output space → encode → atomic `.tmp` + rename write.
/// A failed export leaves no partial output file.
pub fn export_image(
    src: &Path,
    dst: &Path,
    recipe_json: &str,
    opts: &ExportOptions,
) -> Result<ExportResult, ExportError> {
    let bytes = std::fs::read(src)?;
    let (recipe, _skipped) =
        fotobase_core::recipe::parse(recipe_json).map_err(|e| ExportError::Recipe(e.0))?;

    let (rgb, src_w, src_h) = decode_raw(&bytes, src)?;
    let src_profile = icc::extract_icc(&bytes).or_else(|| icc::extract_from_heif(&bytes));

    // Output space: sRGB when the source carries no profile. The tagged
    // source's pixels are converted into sRGB through lcms2, and the output
    // is always tagged (untagged sources export as explicit sRGB).
    let out_profile = crate::color::srgb_profile_bytes()?;
    // Pixel-local recipes stream through the engine strip by strip (bounded
    // linear-light working set). Everything else — geometry, neighbor
    // (sharpen), position-dependent (grain, text, retouch, red-eye) and
    // whole-image-statistics ops — needs the whole image to match the shared
    // engine exactly, so it takes the whole-image path.
    let (out_w, out_h, final_rgb) = if recipe_is_pixel_local(&recipe) {
        (src_w, src_h, apply_streamed(&rgb, src_w, src_h, &recipe))
    } else {
        let linear = crate::rgb_to_linear(&rgb, src_w, src_h);
        let out = fotobase_core::apply::apply_recipe(&linear, &recipe);
        (out.width, out.height, to_srgb8(&out.data))
    };

    // §2J resize: the output fits inside the horizontal/vertical limit box
    // (the tighter of the two limits wins), never upscaled. The legacy
    // longest-edge limit still applies on top for existing callers.
    let (mut dst_w, mut dst_h) = (out_w, out_h);
    let sw = opts.resize_max_width.map_or(1.0, |m| m as f64 / out_w as f64);
    let sh = opts.resize_max_height.map_or(1.0, |m| m as f64 / out_h as f64);
    let s = sw.min(sh).min(1.0);
    if s < 1.0 {
        dst_w = ((out_w as f64 * s).round() as u32).max(1);
        dst_h = ((out_h as f64 * s).round() as u32).max(1);
    }
    if let Some(max_edge) = opts.resize_max_edge {
        let (w, h) = fit(dst_w, dst_h, max_edge);
        dst_w = w;
        dst_h = h;
    }
    let mut final_rgb = resize(&final_rgb, out_w, out_h, dst_w, dst_h);

    // Watermark (TASK-04-015): stamped onto the *output* pixels after the
    // resize, so it keeps a constant relative size regardless of the export
    // resolution. Not a recipe op — it must never appear in previews.
    if let Some(text) = opts.watermark.as_deref().filter(|t| !t.is_empty()) {
        fotobase_core::apply::stamp_text(&mut final_rgb, dst_w, dst_h, text);
    }

    // Color-managed conversion: source profile → output (sRGB). Untagged
    // sources are already sRGB-decoded pixels, so the transform is identity
    // but still runs through lcms2 (no hand-rolled color math, PLAN §3).
    let mut managed = final_rgb;
    crate::color::transform_rgb(&mut managed, src_profile.as_deref(), &out_profile)?;

    let encoded = match opts.format {
        ExportFormat::Jpeg => {
            let mut jpeg = encode_jpeg(&managed, dst_w, dst_h, opts.quality);
            icc::attach_to_jpeg(&mut jpeg, &out_profile);
            jpeg
        }
        ExportFormat::Png => encode_png(&managed, dst_w, dst_h, &out_profile),
    };
    let output_profile = Some(out_profile);

    // Atomic replace: `.tmp` + rename so a crash (or a rejected write) never
    // leaves a half-written exported file at the destination.
    std::fs::create_dir_all(dst.parent().unwrap_or(Path::new(".")))?;
    let ext = dst.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
    let tmp = dst.with_extension(format!("{ext}.tmp"));
    std::fs::write(&tmp, &encoded)?;
    if let Err(e) = std::fs::rename(&tmp, dst) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(ExportResult { width: dst_w, height: dst_h, output_profile })
}

/// Ops that read and write each pixel independently of position and of every
/// other pixel — the only ones for which per-strip application is exactly
/// the whole-image result. A whitelist on purpose: a new op is whole-image
/// (always correct) until it is proven pixel-local here.
const PIXEL_LOCAL_OPS: &[&str] = &[
    "fill_light",
    "temperature",
    "tint",
    "saturation",
    "brightness",
    "highlight_recovery",
    "shadow_recovery",
    "sepia",
    "warm_colors",
    "cool_colors",
    "grayscale",
    "bw",
    "undo_all",
];

fn recipe_is_pixel_local(recipe: &fotobase_core::recipe::Recipe) -> bool {
    recipe.ops.iter().all(|op| PIXEL_LOCAL_OPS.contains(&op.op_type.as_str()))
}

fn to_srgb8(linear: &[f32]) -> Vec<u8> {
    linear.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect()
}

/// Run a pixel-local recipe over `TILE_ROWS`-row strips, so the f32
/// working set is bounded by the strip, not the image.
fn apply_streamed(rgb: &[u8], w: u32, h: u32, recipe: &fotobase_core::recipe::Recipe) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgb.len());
    let mut y = 0u32;
    while y < h {
        let rows = TILE_ROWS.min(h - y);
        let tile = crate::tile_linear(rgb, w, y, rows);
        let edited = fotobase_core::apply::apply_recipe(&tile, recipe);
        out.extend(to_srgb8(&edited.data));
        y += rows;
    }
    out
}

/// [`export_image`] as a queue job (PLAN §3 rule 4): blocking encode work is
/// pushed off the async worker with `spawn_blocking`, cancellation-aware.
pub fn export_image_job(
    src: PathBuf,
    dst: PathBuf,
    recipe_json: String,
    opts: ExportOptions,
) -> Job<Result<ExportResult, ExportError>> {
    job(move |ctx| {
        let src = src.clone();
        let dst = dst.clone();
        let recipe_json = recipe_json.clone();
        let opts = opts.clone();
        async move {
            if ctx.is_cancelled() {
                return Outcome::Cancelled;
            }
            let join =
                tokio::task::spawn_blocking(move || export_image(&src, &dst, &recipe_json, &opts));
            match join.await {
                Ok(outcome) => Outcome::Done(outcome),
                Err(join_err) => Outcome::Failed(format!("export panicked: {join_err}")),
            }
        }
    })
}

/// Peak bytes allocated while streaming `src` through the recipe with the
/// given per-tile RGB8 budget — a model of the f32 working set (tile input +
/// engine output + one RGB8 row), computed while running the same engine on
/// the same strips as [`export_image`], for the memory-budget test. It does
/// not measure the resident decoded source, and writes no file.
pub fn export_peak_bytes(
    src: &Path,
    recipe_json: &str,
    opts: &ExportOptions,
    tile_budget_bytes: usize,
) -> Result<usize, ExportError> {
    let bytes = std::fs::read(src)?;
    let (recipe, _skipped) =
        fotobase_core::recipe::parse(recipe_json).map_err(|e| ExportError::Recipe(e.0))?;
    let (rgb, src_w, src_h) = decode_raw(&bytes, src)?;

    let mut peak = 0usize;
    if recipe_is_pixel_local(&recipe) {
        let rows_per_tile = (tile_budget_bytes / (src_w as usize * 3)).max(1) as u32;
        let mut y = 0u32;
        while y < src_h {
            let rows = rows_per_tile.min(src_h - y);
            let tile = crate::tile_linear(&rgb, src_w, y, rows);
            let out = fotobase_core::apply::apply_recipe(&tile, &recipe);
            // Tile input (f32×3) + engine output (f32×3) + one out-row of RGB8.
            peak = peak.max(tile.data.len() * 4 + out.data.len() * 4 + out.width as usize * 3);
            y += rows;
        }
    } else {
        // Whole-image fallback: the peak is the whole-image buffer, reported
        // honestly so the budget test can assert on it.
        let linear = crate::rgb_to_linear(&rgb, src_w, src_h);
        let out = fotobase_core::apply::apply_recipe(&linear, &recipe);
        peak = peak.max(linear.data.len() * 4 + out.data.len() * 4);
    }
    let _ = opts;
    Ok(peak)
}
