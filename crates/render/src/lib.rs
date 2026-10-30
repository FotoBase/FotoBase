//! Decode, recipe ops, thumbnails (PLAN §3, §4).
//!
//! TASK-01-010 scope: the fast decode path (zune-jpeg + `image`) for JPEG/PNG
//! (further formats land with 01-012), the thumbnail pipeline running inside
//! the bounded job queue, and an on-disk cache keyed by content hash so
//! re-scans never re-render (§5: derived data, never written to sidecars).
//!
//! TASK-01-012 scope: GIF/BMP/TIFF/WebP via `image`, HEIC/AVIF via
//! `libheif-rs` (behind the default-on `heif` cargo feature; the dependency
//! is target-gated to non-Windows until a vcpkg-based packaging leg exists).
//! PSD stays deferred to P2 (task spec: "only if cheap") — it is recognized
//! in the sniff table and reported as unsupported.
//!
//! Deviation from the TASK-01-010 text (recorded in TASK-01-010): resize uses
//! `fast_image_resize` alone. The workspace denies `unsafe_code`, which rules
//! out libvips FFI bindings, and libvips would add a system dependency to
//! every CI platform. Resizing is only a re-encode of the decoded buffer
//! here — the recipe engine (01-045/04-05) owns the linear-light ops.

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-render";

/// Whether the libheif-backed HEIC/AVIF decode path is compiled in.
/// False when the `heif` feature is off or on platforms without the
/// libheif-rs dependency (Windows, until a vcpkg packaging leg lands).
pub const HEIF_AVAILABLE: bool = cfg!(all(feature = "heif", not(target_os = "windows")));

use std::path::{Path, PathBuf};

use fotobase_jobs::{job, Job, Outcome};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub mod color;
pub mod icc;

pub use color::{
    rgb_profile_bytes, srgb_profile_bytes, thumb_profile_tag, transform_rgb, ColorError,
};
pub use icc::extract_icc;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("decode failed for {path}: {detail}")]
    Decode { path: PathBuf, detail: String },
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Thumbnail sizing options.
#[derive(Debug, Clone)]
pub struct ThumbOptions {
    /// Longest edge of the produced thumbnail, in pixels. Sources smaller
    /// than this are never upscaled.
    pub max_edge: u32,
    /// JPEG quality of the cached thumbnail.
    pub quality: u8,
    /// Optional cache-file tag distinguishing a size variant from the
    /// legacy 512px entry (TASK-01-024 HiDPI: `Some("@2x")` names
    /// `<hash>@2x.jpg`). `None` keeps the legacy `<hash>.jpg` name, so
    /// existing caches stay valid.
    pub cache_tag: Option<String>,
    /// Display ICC profile the pixels are transformed into before the cache
    /// write (TASK-02-001). `Some` on macOS/Windows (the OS display profile)
    /// and on Linux (the bundled sRGB profile); `None` keeps the legacy
    /// untagged, untransformed pipeline. The profile fingerprint is appended
    /// to the cache-file name, so a profile switch re-renders instead of
    /// reusing pixels transformed for another display.
    pub display_profile: Option<Vec<u8>>,
}

impl Default for ThumbOptions {
    fn default() -> Self {
        Self { max_edge: 512, quality: 85, cache_tag: None, display_profile: None }
    }
}

/// A rendered (or cache-hit) thumbnail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    /// Path of the cached thumbnail file.
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    /// True when the thumbnail came from the on-disk cache; the source was
    /// not decoded.
    pub from_cache: bool,
}

enum Sniffed {
    Jpeg,
    Png,
    Gif,
    Bmp,
    Tiff,
    Webp,
    /// HEIF container (HEIC/HEVC brands and AVIF/AV1 brands alike); decoded
    /// through libheif-rs when the `heif` feature is compiled in.
    Heif,
    /// Recognized but deferred: PSD thumbnails land with P2 (RAW/JXL).
    Unsupported(&'static str),
    /// No magic we know — treated as a decode failure, not a format gap.
    Unknown,
}

fn sniff(b: &[u8]) -> Sniffed {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Sniffed::Jpeg
    } else if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        Sniffed::Png
    } else if b.starts_with(b"GIF8") {
        Sniffed::Gif
    } else if b.starts_with(b"BM") && b.len() > 2 {
        Sniffed::Bmp
    } else if b.starts_with(&[0x49, 0x49, 0x2A, 0x00]) || b.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]) {
        Sniffed::Tiff
    } else if b.len() > 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP" {
        Sniffed::Webp
    } else if b.len() > 12 && &b[4..8] == b"ftyp" {
        // ISOBMFF: the major brand at [8..12] covers heic/heix/hevc/mif1
        // (HEIF family) and avif/avis (AVIF). Distinguishing brands is not
        // needed — libheif parses the container itself.
        Sniffed::Heif
    } else if b.starts_with(b"8BPS") {
        Sniffed::Unsupported("PSD")
    } else {
        Sniffed::Unknown
    }
}

/// Render (or fetch from cache) the thumbnail for `src`.
///
/// Cache layout: `<cache_dir>/<sha256-first-32-hex>.jpg`. The key is the
/// content hash of the source file, so a re-scan of an unchanged library is
/// all cache hits and a sidecar-affecting edit invalidates automatically.
pub fn thumbnail(
    src: &Path,
    cache_dir: &Path,
    opts: ThumbOptions,
) -> Result<Thumbnail, RenderError> {
    let bytes = std::fs::read(src)?;
    let hash: String = Sha256::digest(&bytes)[..16].iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    });

    let profile_tag = color::thumb_profile_tag(opts.display_profile.as_deref());
    let cached = match (&opts.cache_tag, profile_tag.is_empty()) {
        (Some(tag), _) => cache_dir.join(format!("{hash}{tag}{profile_tag}.jpg")),
        (None, false) => cache_dir.join(format!("{hash}{profile_tag}.jpg")),
        (None, true) => cache_dir.join(format!("{hash}.jpg")),
    };
    if let Some((w, h)) = cached_dimensions(&cached) {
        return Ok(Thumbnail { path: cached, width: w, height: h, from_cache: true });
    }

    let (mut rgb, src_w, src_h) = decode(&bytes, src)?;
    if let Some(dst) = opts.display_profile.as_deref() {
        let src_profile = icc::extract_icc(&bytes).or_else(|| icc::extract_from_heif(&bytes));
        transform_rgb(&mut rgb, src_profile.as_deref(), dst)
            .map_err(|e| RenderError::Decode { path: src.to_path_buf(), detail: e.to_string() })?;
    }
    let (dst_w, dst_h) = fit(src_w, src_h, opts.max_edge);
    let resized = resize(&rgb, src_w, src_h, dst_w, dst_h);
    let mut encoded = encode_jpeg(&resized, dst_w, dst_h, opts.quality);
    if let Some(profile) = &opts.display_profile {
        // Tag the output with the destination profile so the WebView knows
        // the pixels are already in display space and applies no second
        // transform (the "no double-transform" requirement, sRGB Linux
        // fallback included).
        icc::attach_to_jpeg(&mut encoded, profile);
    }

    std::fs::create_dir_all(cache_dir)?;
    // Atomic replace (same discipline as the sidecar writer): `.tmp` + rename
    // so a crash never leaves a half-written cache entry.
    let tmp = cache_dir.join(format!("{hash}.jpg.tmp"));
    std::fs::write(&tmp, &encoded)?;
    if let Err(e) = std::fs::rename(&tmp, &cached) {
        let _ = std::fs::remove_file(&tmp);
        if !cached.exists() {
            return Err(e.into());
        }
        // Lost a race with another worker that wrote the same entry: fine.
    }
    Ok(Thumbnail { path: cached, width: dst_w, height: dst_h, from_cache: false })
}

/// Cheap dimension read of a cached thumbnail; `None` when absent/unreadable
/// (a corrupt entry is treated as a miss and re-rendered).
fn cached_dimensions(p: &Path) -> Option<(u32, u32)> {
    image::ImageReader::open(p).ok()?.into_dimensions().ok()
}

fn decode(bytes: &[u8], src: &Path) -> Result<(Vec<u8>, u32, u32), RenderError> {
    match sniff(bytes) {
        Sniffed::Jpeg => match zune_jpeg_decode(bytes) {
            Ok(out) => Ok(out),
            Err(detail) => Err(RenderError::Decode { path: src.to_path_buf(), detail }),
        },
        // GIF/BMP/TIFF/WebP (and PNG fallback) through the `image` crate.
        Sniffed::Png | Sniffed::Gif | Sniffed::Bmp | Sniffed::Tiff | Sniffed::Webp => {
            match decode_with_image(bytes) {
                Ok(out) => Ok(out),
                Err(detail) => Err(RenderError::Decode { path: src.to_path_buf(), detail }),
            }
        }
        Sniffed::Heif => match heif::decode_heif(bytes) {
            Ok(out) => Ok(out),
            Err(detail) => Err(RenderError::Decode { path: src.to_path_buf(), detail }),
        },
        Sniffed::Unsupported(fmt) => Err(RenderError::UnsupportedFormat(fmt.into())),
        Sniffed::Unknown => Err(RenderError::Decode {
            path: src.to_path_buf(),
            detail: "unrecognized image format".into(),
        }),
    }
}

fn decode_with_image(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    use image::GenericImageView;
    let img = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("{e}"))?
        .decode()
        .map_err(|e| e.to_string())?;
    let (w, h) = img.dimensions();
    Ok((img.to_rgb8().into_raw(), w, h))
}

fn zune_jpeg_decode(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    use zune_jpeg::zune_core::{colorspace::ColorSpace, options::DecoderOptions};
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(bytes, options);
    decoder.decode_headers().map_err(|e| format!("{e}"))?;
    let info = decoder.info().ok_or("no jpeg header info")?;
    let (w, h) = (info.width as u32, info.height as u32);
    let pixels = decoder.decode().map_err(|e| format!("{e}"))?;
    Ok((pixels, w, h))
}

/// HEIC/AVIF decode through libheif-rs (PLAN §4).
#[cfg(all(feature = "heif", not(target_os = "windows")))]
mod heif {
    use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

    /// Decode the primary image of a HEIF/AVIF container into packed RGB8.
    pub(super) fn decode_heif(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
        let ctx = HeifContext::read_from_bytes(bytes).map_err(|e| format!("{e}"))?;
        let handle = ctx.primary_image_handle().map_err(|e| format!("{e}"))?;
        let (w, h) = (handle.width(), handle.height());
        if w == 0 || h == 0 {
            return Err("empty heif image".into());
        }
        let lib = LibHeif::new();
        let img = lib
            .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgb), None)
            .map_err(|e| format!("{e}"))?;
        let plane = img
            .planes()
            .interleaved
            .ok_or_else(|| "heif image has no interleaved plane".to_string())?;
        if plane.width != w || plane.height != h {
            return Err(format!(
                "heif plane {}×{} differs from handle {w}×{h}",
                plane.width, plane.height
            ));
        }
        // libheif returns row-strided buffers; repack into packed RGB.
        let stride = plane.stride;
        let row_len = w as usize * 3;
        let mut rgb = Vec::with_capacity(row_len * h as usize);
        for y in 0..h as usize {
            let row = plane
                .data
                .get(y * stride..y * stride + row_len)
                .ok_or_else(|| "heif plane data shorter than stride implies".to_string())?;
            rgb.extend_from_slice(row);
        }
        Ok((rgb, w, h))
    }
}

/// Stub for targets without the libheif dependency (Windows for now): the
/// sniff table still recognizes the container and reports it as unsupported
/// instead of misreading it as corrupt.
#[cfg(any(not(feature = "heif"), target_os = "windows"))]
mod heif {
    pub(super) fn decode_heif(_bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
        Err("HEIC/AVIF support is not compiled in for this platform".into())
    }
}

/// Fit inside `max_edge × max_edge`, never upscaling.
fn fit(w: u32, h: u32, max_edge: u32) -> (u32, u32) {
    let longest = w.max(h);
    if longest <= max_edge || longest == 0 {
        return (w, h);
    }
    let scale = f64::from(max_edge) / f64::from(longest);
    let dw = ((f64::from(w) * scale).round() as u32).max(1);
    let dh = ((f64::from(h) * scale).round() as u32).max(1);
    (dw, dh)
}

fn resize(rgb: &[u8], w: u32, h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    if (w, h) == (dst_w, dst_h) {
        return rgb.to_vec();
    }
    use fast_image_resize::{images::Image, PixelType, ResizeAlg, ResizeOptions, Resizer};
    let src =
        Image::from_vec_u8(w, h, rgb.to_vec(), PixelType::U8x3).expect("buffer matches dimensions");
    let mut dst = Image::new(dst_w, dst_h, PixelType::U8x3);
    let options = ResizeOptions::new()
        .resize_alg(ResizeAlg::Convolution(fast_image_resize::FilterType::Lanczos3));
    Resizer::new().resize(&src, &mut dst, &options).expect("resize with valid dimensions");
    dst.buffer().to_vec()
}

fn encode_jpeg(rgb: &[u8], w: u32, h: u32, quality: u8) -> Vec<u8> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    enc.write_image(rgb, w, h, image::ExtendedColorType::Rgb8).expect("encoding in-memory jpeg");
    out
}

/// [`thumbnail`] as a queue job (PLAN §3 rule 4): cooperative cancellation
/// before the decode starts; errors are results, not queue failures, so one
/// malformed photo never takes down the batch (§9).
pub fn thumbnail_job(
    src: PathBuf,
    cache_dir: PathBuf,
    opts: ThumbOptions,
) -> Job<Result<Thumbnail, RenderError>> {
    job(move |ctx| {
        let src = src.clone();
        let cache_dir = cache_dir.clone();
        let opts = opts.clone();
        async move {
            if ctx.is_cancelled() {
                return Outcome::Cancelled;
            }
            Outcome::Done(thumbnail(&src, &cache_dir, opts))
        }
    })
}

/// Render (or fetch from cache) the thumbnail for a video file
/// (TASK-02-004, PLAN §2 Video strategy thumbnails bullet).
///
/// A rendered (or cache-hit) recipe preview (TASK-04-010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipePreview {
    /// Path of the cached preview file (JPEG).
    pub path: PathBuf,
    /// True when served from the on-disk cache; the source was not decoded.
    pub from_cache: bool,
}

/// Stable cache-key suffix for a recipe: first 16 bytes of the recipe
/// JSON's SHA-256, lowercase hex. Two different recipes never share a
/// cache entry; the same recipe text always does.
pub fn recipe_id(recipe_json: &str) -> String {
    let digest = Sha256::digest(recipe_json.as_bytes());
    digest[..16].iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// Render (or fetch from cache) the edited preview for `src` under
/// `recipe_json` (TASK-04-010, PLAN §2D live preview, §3 rules 3–5).
///
/// Pipeline: decode → linear-light buffer → the shared recipe engine
/// (the exact code the golden tests pin and export reuses) → resize to
/// `max_edge` (never upscaling) → JPEG → atomic `.tmp` + rename cache
/// write. Cache layout: `<cache_dir>/edit/<content-hash>-r<recipe-id>.jpg`.
///
/// Unlike thumbnails, no display-ICC transform is applied: the edit view
/// samples and adjusts pixels, so baking a display transform into the
/// preview would corrupt the edit. The preview is untransformed sRGB.
///
/// A malformed recipe is an error — the edit view must never silently show
/// the original for a recipe it could not read. Unknown *op types* inside a
/// well-formed packet are skipped per the reader conformance rule
/// (edit-recipe spec §5).
pub fn recipe_preview(
    src: &Path,
    cache_dir: &Path,
    recipe_json: &str,
    max_edge: u32,
    quality: u8,
) -> Result<RecipePreview, RenderError> {
    let bytes = std::fs::read(src)?;
    let hash: String = Sha256::digest(&bytes)[..16].iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    });
    let id = recipe_id(recipe_json);
    let dir = cache_dir.join("edit");
    let cached = dir.join(format!("{hash}-r{id}.jpg"));
    if cached_dimensions(&cached).is_some() {
        return Ok(RecipePreview { path: cached, from_cache: true });
    }

    let (recipe, _skipped) =
        fotobase_core::recipe::parse(recipe_json).map_err(|e| RenderError::Decode {
            path: src.to_path_buf(),
            detail: format!("malformed edit recipe: {}", e.0),
        })?;

    let (rgb, src_w, src_h) = decode(&bytes, src)?;
    // The pipeline treats decoded 8-bit values as the engine's [0, 1]
    // domain (same convention as the golden harness, which compares
    // engine output on 0–255-encoded PNGs).
    let linear = fotobase_core::buffer::LinearImage {
        width: src_w,
        height: src_h,
        data: rgb.iter().map(|b| f32::from(*b) / 255.0).collect(),
    };
    let out = fotobase_core::apply::apply_recipe(&linear, &recipe);
    let srgb: Vec<u8> =
        out.data.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect();
    let (dst_w, dst_h) = fit(out.width, out.height, max_edge);
    let resized = resize(&srgb, out.width, out.height, dst_w, dst_h);
    let encoded = encode_jpeg(&resized, dst_w, dst_h, quality);

    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!("{hash}-r{id}.jpg.tmp"));
    std::fs::write(&tmp, &encoded)?;
    if let Err(e) = std::fs::rename(&tmp, &cached) {
        let _ = std::fs::remove_file(&tmp);
        if !cached.exists() {
            return Err(e.into());
        }
        // Lost a race with another worker that wrote the same entry: fine.
    }
    Ok(RecipePreview { path: cached, from_cache: false })
}

/// [`recipe_preview`] as a queue job: visible-first priority, coalescing on
/// the caller's key, cancellation-aware (PLAN §3 rule 4).
pub fn recipe_preview_job(
    src: PathBuf,
    cache_dir: PathBuf,
    recipe_json: String,
    max_edge: u32,
    quality: u8,
) -> Job<Result<RecipePreview, RenderError>> {
    job(move |ctx| {
        let src = src.clone();
        let cache_dir = cache_dir.clone();
        let recipe_json = recipe_json.clone();
        async move {
            if ctx.is_cancelled() {
                return Outcome::Cancelled;
            }
            Outcome::Done(recipe_preview(&src, &cache_dir, &recipe_json, max_edge, quality))
        }
    })
}

/// One frame is extracted at ~10% of the clip's duration and fed through the
/// exact same resize/encode/cache pipeline as images — same cache-key scheme
/// (the content hash of the *video* file, matching the catalog's stored hash
/// and the `thumb://` key), same `<hash>.jpg` layout, same atomic `.tmp` +
/// rename write. No new serve path: the existing `thumb://` handler serves
/// video thumbnails indistinguishably from image ones.
///
/// A failed extraction (corrupt/unsupported media, missing ffmpeg) is an
/// [`Err`] — the scheme layer serves the placeholder instead of an error
/// dialog, and the cache is left untouched so a fixed file can be re-rendered.
pub fn video_thumbnail(
    src: &Path,
    cache_dir: &Path,
    opts: ThumbOptions,
    extractor: &dyn fotobase_video::FrameExtractor,
) -> Result<Thumbnail, RenderError> {
    // Same key derivation as the catalog (`content_hash`): streamed SHA-256,
    // first 16 bytes — camera cards hold multi-gigabyte clips, nothing is
    // read whole.
    use sha2::Digest;
    let mut file = std::fs::File::open(src)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let hash: String = hasher.finalize()[..16].iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    });

    let profile_tag = color::thumb_profile_tag(opts.display_profile.as_deref());
    let cached = match (&opts.cache_tag, profile_tag.is_empty()) {
        (Some(tag), _) => cache_dir.join(format!("{hash}{tag}{profile_tag}.jpg")),
        (None, false) => cache_dir.join(format!("{hash}{profile_tag}.jpg")),
        (None, true) => cache_dir.join(format!("{hash}.jpg")),
    };
    if let Some((w, h)) = cached_dimensions(&cached) {
        return Ok(Thumbnail { path: cached, width: w, height: h, from_cache: true });
    }

    // Extract one frame next to the cache, then decode it through the plain
    // image pipeline (the frame is a JPEG — the sniff table handles it). The
    // cache directory must exist before the extractor writes into it.
    std::fs::create_dir_all(cache_dir)?;
    let frame = cache_dir.join(format!("{hash}.frame.jpg"));
    extractor
        .extract_frame(src, &frame)
        .map_err(|e| RenderError::Decode { path: src.to_path_buf(), detail: e.to_string() })?;
    let frame_bytes = match std::fs::read(&frame) {
        Ok(bytes) => bytes,
        Err(e) => {
            let _ = std::fs::remove_file(&frame);
            return Err(e.into());
        }
    };
    let _ = std::fs::remove_file(&frame);

    let (mut rgb, src_w, src_h) = decode(&frame_bytes, src)?;
    if let Some(dst) = opts.display_profile.as_deref() {
        // Extracted frames carry no ICC profile; ffmpeg wrote sRGB pixels, so
        // the transform runs with no embedded source profile (same as an
        // untagged image).
        transform_rgb(&mut rgb, None, dst)
            .map_err(|e| RenderError::Decode { path: src.to_path_buf(), detail: e.to_string() })?;
    }
    let (dst_w, dst_h) = fit(src_w, src_h, opts.max_edge);
    let resized = resize(&rgb, src_w, src_h, dst_w, dst_h);
    let mut encoded = encode_jpeg(&resized, dst_w, dst_h, opts.quality);
    if let Some(profile) = &opts.display_profile {
        icc::attach_to_jpeg(&mut encoded, profile);
    }

    let tmp = cache_dir.join(format!("{hash}.jpg.tmp"));
    std::fs::write(&tmp, &encoded)?;
    if let Err(e) = std::fs::rename(&tmp, &cached) {
        let _ = std::fs::remove_file(&tmp);
        if !cached.exists() {
            return Err(e.into());
        }
        // Lost a race with another worker that wrote the same entry: fine.
    }
    Ok(Thumbnail { path: cached, width: dst_w, height: dst_h, from_cache: false })
}

/// [`video_thumbnail`] as a queue job (PLAN §3 rule 4): video thumbnails run
/// in the *background* priority class — below every visible-image thumbnail,
/// never blocking the grid — with cooperative cancellation before the
/// extraction starts. Errors are results, not queue failures (§9).
pub fn video_thumbnail_job(
    src: PathBuf,
    cache_dir: PathBuf,
    opts: ThumbOptions,
    extractor: std::sync::Arc<dyn fotobase_video::FrameExtractor + Send + Sync>,
) -> Job<Result<Thumbnail, RenderError>> {
    job(move |ctx| {
        let src = src.clone();
        let cache_dir = cache_dir.clone();
        let opts = opts.clone();
        let extractor = extractor.clone();
        async move {
            if ctx.is_cancelled() {
                return Outcome::Cancelled;
            }
            // ffmpeg extraction is blocking work (a subprocess run that can
            // take hundreds of ms): push it off the async worker so a video
            // thumbnail never stalls other queued jobs on the same runtime.
            let join = tokio::task::spawn_blocking(move || {
                video_thumbnail(&src, &cache_dir, opts, extractor.as_ref())
            });
            match join.await {
                Ok(outcome) => Outcome::Done(outcome),
                // The blocking task only panics if the closure panicked; a
                // panicking extractor is a failed job, not a queue failure.
                Err(join_err) => Outcome::Failed(format!("video thumbnail panicked: {join_err}")),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_identity() {
        assert_eq!(super::CRATE_NAME, "fotobase-render");
    }

    #[test]
    fn fit_never_upscales_and_preserves_aspect() {
        assert_eq!(fit(400, 200, 100), (100, 50));
        assert_eq!(fit(80, 60, 512), (80, 60));
        assert_eq!(fit(1000, 1000, 100), (100, 100));
        assert_eq!(fit(3, 7, 5), (2, 5));
        assert_eq!(fit(0, 0, 100), (0, 0));
    }

    #[test]
    fn sniff_detects_known_magics() {
        assert!(matches!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Sniffed::Jpeg));
        assert!(matches!(sniff(&[0x89, b'P', b'N', b'G', 0x0D]), Sniffed::Png));
        assert!(matches!(sniff(b"GIF89a"), Sniffed::Gif));
        assert!(matches!(sniff(b"BM\x36\x00"), Sniffed::Bmp));
        assert!(matches!(sniff(&[0x49, 0x49, 0x2A, 0x00, 0x08]), Sniffed::Tiff));
        assert!(matches!(sniff(&[0x4D, 0x4D, 0x00, 0x2A, 0x00]), Sniffed::Tiff));
        assert!(matches!(sniff(b"RIFFxxxxWEBPVP8 "), Sniffed::Webp));
        assert!(matches!(sniff(b"xxxxftypheicxxxx"), Sniffed::Heif));
        assert!(matches!(sniff(b"xxxxftypavifxxxx"), Sniffed::Heif));
        assert!(matches!(sniff(b"8BPS\x00\x01"), Sniffed::Unsupported("PSD")));
        assert!(matches!(sniff(b"plain text"), Sniffed::Unknown));
    }

    #[test]
    fn heif_availability_flag_matches_cfg() {
        assert_eq!(HEIF_AVAILABLE, cfg!(all(feature = "heif", not(target_os = "windows"))));
    }
}
