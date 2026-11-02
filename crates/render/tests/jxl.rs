//! Integration tests for JPEG XL decoding (TASK-04-014, TDD).
//!
//! Per PLAN §4: JXL via `jxl-oxide` (pure Rust, so the feature compiles on
//! all three OSes). JXL must decode through the same dispatch as every other
//! format — thumbnail pipeline and recipe preview both consume `decode()`,
//! so passing these tests means no special cases anywhere.
//!
//! Fixture: the tiny committed sample under `tools/fixture-samples/`
//! (ADR-0009 pattern: produced once with `cjxl`, committed, never
//! regenerated in CI — jxl-oxide is decode-only so no encoder exists in the
//! Rust tree).
//!
//! Robustness (§9): truncated/malformed JXL bytes must error, never panic.

use std::path::{Path, PathBuf};

use fotobase_render::{recipe_preview, thumbnail, RenderError, ThumbOptions};
use tempfile::TempDir;

fn fixture_samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/fixture-samples")
}

fn jxl_available() -> bool {
    fotobase_render::JXL_AVAILABLE
}

/// Assert a source of `w × h` produces a thumbnail of the same size (no
/// upscale, and nothing to shrink below the default 512 max edge).
fn assert_thumb_dims(src: &Path, w: u32, h: u32) {
    let tmp = TempDir::new().unwrap();
    let t = thumbnail(src, tmp.path(), ThumbOptions::default()).unwrap();
    assert_eq!((t.width, t.height), (w, h), "wrong thumbnail dims for {}", src.display());
    assert!(!t.from_cache);
    let img = image::ImageReader::open(&t.path).unwrap().decode().unwrap();
    assert_eq!((img.width(), img.height()), (w, h));
}

#[test]
fn jxl_sample_decodes_to_thumbnail() {
    if !jxl_available() {
        eprintln!("skipping: jxl support not compiled in");
        return;
    }
    let src = fixture_samples().join("sample-1.jxl");
    assert_thumb_dims(&src, 64, 48);
}

#[test]
fn jxl_truncated_returns_decode_error_not_panic() {
    if !jxl_available() {
        eprintln!("skipping: jxl support not compiled in");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let full = std::fs::read(fixture_samples().join("sample-1.jxl")).unwrap();
    let p = tmp.path().join("trunc.jxl");
    // Keep the container magic (a real truncated file still sniffs as JXL)
    // but cut the body.
    std::fs::write(&p, &full[..full.len() / 2]).unwrap();

    let err = thumbnail(&p, &tmp.path().join("thumbs"), ThumbOptions::default()).unwrap_err();
    assert!(matches!(err, RenderError::Decode { .. }), "{err}");
}

#[test]
fn jxl_garbage_codestream_returns_decode_error_not_panic() {
    if !jxl_available() {
        eprintln!("skipping: jxl support not compiled in");
        return;
    }
    let tmp = TempDir::new().unwrap();
    // Raw codestream signature 0xFF 0x0A followed by junk.
    let p = tmp.path().join("junk.jxl");
    std::fs::write(&p, [0xFF, 0x0A, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05]).unwrap();

    let err = thumbnail(&p, &tmp.path().join("thumbs"), ThumbOptions::default()).unwrap_err();
    assert!(matches!(err, RenderError::Decode { .. }), "{err}");
}

#[test]
fn jxl_recipe_preview_applies_recipe_at_full_resolution() {
    if !jxl_available() {
        eprintln!("skipping: jxl support not compiled in");
        return;
    }
    let src = fixture_samples().join("sample-1.jxl");
    let tmp = TempDir::new().unwrap();
    // max_edge larger than the source: `fit` never upscales, so the preview
    // comes out at the full source resolution with the recipe applied —
    // the same shared pipeline export reuses (04-018).
    let p = recipe_preview(
        &src,
        tmp.path(),
        r#"{"spec":"fotobase.recipe","version":1,"ops":[]}"#,
        4096,
        90,
    )
    .unwrap();
    let img = image::ImageReader::open(&p.path).unwrap().decode().unwrap();
    assert_eq!((img.width(), img.height()), (64, 48));
    assert!(!p.from_cache);
}
