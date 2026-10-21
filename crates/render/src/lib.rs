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
}

impl Default for ThumbOptions {
    fn default() -> Self {
        Self { max_edge: 512, quality: 85 }
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

    let cached = cache_dir.join(format!("{hash}.jpg"));
    if let Some((w, h)) = cached_dimensions(&cached) {
        return Ok(Thumbnail { path: cached, width: w, height: h, from_cache: true });
    }

    let (rgb, src_w, src_h) = decode(&bytes, src)?;
    let (dst_w, dst_h) = fit(src_w, src_h, opts.max_edge);
    let resized = resize(&rgb, src_w, src_h, dst_w, dst_h);
    let encoded = encode_jpeg(&resized, dst_w, dst_h, opts.quality);

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
