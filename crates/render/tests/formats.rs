//! Integration tests for format support (TASK-01-012, TDD).
//!
//! Per PLAN §2A/§4: JPEG/PNG/GIF/BMP/TIFF/WebP via `image`, HEIC/AVIF via
//! libheif-rs. GIF/BMP/TIFF/WebP fixtures are generated in-process with the
//! `image` encoders (same trick as `pipeline.rs`); HEIC/AVIF use the tiny
//! committed samples under `tools/fixture-samples/` (ADR-0009 pattern: no
//! libheif *encoder* toolchain in CI — decoding needs a libheif build, which
//! the `heif` cargo feature provides via libheif-rs' embedded libheif).
//!
//! Robustness (§9): a truncated/corrupt file of every format must come back
//! as a decode error, never a panic.

use std::path::{Path, PathBuf};

use fotobase_render::{thumbnail, RenderError, ThumbOptions};
use tempfile::TempDir;

/// Tiny committed samples under `tools/fixture-samples/` (ADR-0009 pattern:
/// no libheif *encoder* toolchain in CI — decoding needs a libheif build,
/// which the `heif` cargo feature provides via libheif-rs' embedded libheif).
fn fixture_samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/fixture-samples")
}

/// The heif-backed tests only run where the `heif` feature actually compiles
/// (libheif-rs is target-gated to non-Windows; see Cargo.toml).
fn heif_available() -> bool {
    fotobase_render::HEIF_AVAILABLE
}

/// Write an RGB image in the given format and return the path.
fn save(dir: &Path, name: &str, w: u32, h: u32, fmt: image::ImageFormat) -> PathBuf {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let v = (x * 3 + y * 7) % 32;
        image::Rgb([v as u8, (v * 3 % 256) as u8, (v * 7 % 256) as u8])
    });
    let p = dir.join(name);
    img.save_with_format(&p, fmt).unwrap();
    p
}

/// Assert a source of `w × h` produces a thumbnail of the same size (no
/// upscale, and nothing to shrink below the default 512 max edge).
fn assert_thumb_dims(src: &Path, w: u32, h: u32) {
    let tmp = TempDir::new().unwrap();
    let t = thumbnail(src, tmp.path(), ThumbOptions::default()).unwrap();
    assert_eq!((t.width, t.height), (w, h), "wrong thumbnail dims for {}", src.display());
    assert!(!t.from_cache);
    // The cached thumbnail must itself decode as a JPEG of those dimensions.
    let img = image::ImageReader::open(&t.path).unwrap().decode().unwrap();
    assert_eq!((img.width(), img.height()), (w, h));
}

#[test]
fn gif_decodes_to_thumbnail() {
    let tmp = TempDir::new().unwrap();
    let src = save(tmp.path(), "a.gif", 96, 64, image::ImageFormat::Gif);
    assert_thumb_dims(&src, 96, 64);
}

#[test]
fn bmp_decodes_to_thumbnail() {
    let tmp = TempDir::new().unwrap();
    let src = save(tmp.path(), "a.bmp", 96, 64, image::ImageFormat::Bmp);
    assert_thumb_dims(&src, 96, 64);
}

#[test]
fn tiff_decodes_to_thumbnail() {
    let tmp = TempDir::new().unwrap();
    let src = save(tmp.path(), "a.tif", 96, 64, image::ImageFormat::Tiff);
    assert_thumb_dims(&src, 96, 64);
}

#[test]
fn webp_decodes_to_thumbnail() {
    let tmp = TempDir::new().unwrap();
    let src = save(tmp.path(), "a.webp", 96, 64, image::ImageFormat::WebP);
    assert_thumb_dims(&src, 96, 64);
}

#[test]
fn tiff_larger_than_max_edge_is_downscaled() {
    let tmp = TempDir::new().unwrap();
    let src = save(tmp.path(), "big.tif", 400, 200, image::ImageFormat::Tiff);
    let t = thumbnail(
        &src,
        &tmp.path().join("c"),
        ThumbOptions { max_edge: 100, ..Default::default() },
    )
    .unwrap();
    assert_eq!((t.width, t.height), (100, 50));
}

#[test]
fn heic_sample_decodes_to_thumbnail() {
    if !heif_available() {
        eprintln!("skipping: heif support not compiled in");
        return;
    }
    let src = fixture_samples().join("sample-1.heic");
    assert_thumb_dims(&src, 64, 48);
}

#[test]
fn avif_sample_decodes_to_thumbnail() {
    if !heif_available() {
        eprintln!("skipping: heif support not compiled in");
        return;
    }
    let src = fixture_samples().join("sample-1.avif");
    assert_thumb_dims(&src, 64, 48);
}

/// Truncated versions of each generated fixture must error, not panic.
#[test]
fn truncated_files_return_decode_errors_not_panics() {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("thumbs");
    let full = save(tmp.path(), "full.webp", 32, 32, image::ImageFormat::WebP);
    let bytes = std::fs::read(&full).unwrap();

    for fmt in [
        ("gif", image::ImageFormat::Gif),
        ("bmp", image::ImageFormat::Bmp),
        ("tif", image::ImageFormat::Tiff),
        ("webp", image::ImageFormat::WebP),
    ] {
        let p = tmp.path().join(format!("trunc.{}", fmt.0));
        // Keep the magic bytes (a real-world truncated file still sniffs as
        // its format) but cut the body.
        let cut = &bytes[..bytes.len() / 2];
        std::fs::write(&p, cut).unwrap();
        let err = thumbnail(&p, &cache, ThumbOptions::default()).unwrap_err();
        assert!(matches!(err, RenderError::Decode { .. }), "{:?}: {}", fmt.0, err);
    }
}

#[test]
fn truncated_heic_returns_decode_error_not_panic() {
    if !heif_available() {
        eprintln!("skipping: heif support not compiled in");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let full = std::fs::read(fixture_samples().join("sample-1.heic")).unwrap();
    let p = tmp.path().join("trunc.heic");
    std::fs::write(&p, &full[..full.len() / 2]).unwrap();

    let err = thumbnail(&p, &tmp.path().join("thumbs"), ThumbOptions::default()).unwrap_err();
    assert!(matches!(err, RenderError::Decode { .. }), "{err}");
}

#[test]
fn corrupt_avif_returns_decode_error_not_panic() {
    if !heif_available() {
        eprintln!("skipping: heif support not compiled in");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let full = std::fs::read(fixture_samples().join("sample-1.avif")).unwrap();
    let p = tmp.path().join("trunc.avif");
    std::fs::write(&p, &full[..full.len() / 2]).unwrap();

    let err = thumbnail(&p, &tmp.path().join("thumbs"), ThumbOptions::default()).unwrap_err();
    assert!(matches!(err, RenderError::Decode { .. }), "{err}");
}

#[test]
fn psd_is_reported_as_unsupported_deferred() {
    // PSD stays deferred (RAW/JXL arrive in P2): recognized, reported, never
    // a panic.
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("layered.psd");
    std::fs::write(&p, b"8BPS\x00\x00\x00\x00").unwrap();
    let err = thumbnail(&p, &tmp.path().join("thumbs"), ThumbOptions::default()).unwrap_err();
    assert!(matches!(err, RenderError::UnsupportedFormat(ref f) if f == "PSD"));
}
