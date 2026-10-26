//! TASK-02-002: golden-image tests for the display transform (PLAN §6/§9
//! "Render fidelity: golden images").
//!
//! Matrix: 3 source profiles (sRGB, Display-P3, Adobe RGB) × 2 display
//! profiles (sRGB, Display-P3). Each source is a synthetic JPEG with the
//! source profile embedded in APP2, saturated primaries at full sensitivity.
//! The transformed output is compared against a committed reference PNG with
//! a perceptual tolerance (RMS of per-channel deltas), not byte equality —
//! lcms2 versions differ slightly in gamut mapping.
//!
//! Regenerate the golden PNGs with `FB_GOLDEN_REGENERATE=1 cargo test -p
//! fotobase-render --test golden` (then review the diff before committing).

use std::path::{Path, PathBuf};

use fotobase_render::{extract_icc, rgb_profile_bytes, srgb_profile_bytes, transform_rgb};
use image::ImageEncoder;

const FIXTURE_DIR: &str = "tests/fixtures/golden";
/// RMS (per-channel, 0–255) tolerance. lcms2 gamut mapping differs slightly
/// between releases; 3.0 keeps the test stable across versions while still
/// catching real transform regressions (identity measures far above it).
const RMS_TOLERANCE: f64 = 3.0;

fn fixture_dir() -> PathBuf {
    // Cargo sets CWD to the crate root for integration tests.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_DIR)
}

fn source_profile(kind: &str) -> Vec<u8> {
    match kind {
        "srgb" => srgb_profile_bytes().expect("sRGB profile"),
        "display-p3" => rgb_profile_bytes(
            [0.3127, 0.3290, 1.0],
            [[0.680, 0.320, 1.0], [0.265, 0.690, 1.0], [0.150, 0.060, 1.0]],
            2.19921875,
        )
        .expect("Display-P3 profile"),
        "adobe-rgb" => rgb_profile_bytes(
            [0.3127, 0.3290, 1.0],
            [[0.64, 0.33, 1.0], [0.21, 0.71, 1.0], [0.15, 0.06, 1.0]],
            2.19921875,
        )
        .expect("Adobe RGB profile"),
        other => panic!("unknown source profile {other}"),
    }
}

fn display_profile(kind: &str) -> Vec<u8> {
    match kind {
        "srgb" => srgb_profile_bytes().expect("sRGB display profile"),
        "display-p3" => source_profile("display-p3"),
        other => panic!("unknown display profile {other}"),
    }
}

/// Synthetic fixture with saturated primaries — maximum transform
/// sensitivity. Layout per 8×8 block: pure R, G, B, C, M, Y, white, a
/// 25/50/75% gray ramp.
fn fixture_pixels() -> Vec<u8> {
    let blocks: [&[u8; 3]; 8] = [
        &[255, 0, 0],
        &[0, 255, 0],
        &[0, 0, 255],
        &[0, 255, 255],
        &[255, 0, 255],
        &[255, 255, 0],
        &[255, 255, 255],
        &[64, 64, 64],
    ];
    let mut px = Vec::with_capacity(8 * 8 * 8 * 8 * 3);
    for _by in 0..8usize {
        for y in 0..8usize {
            for &color0 in &blocks {
                let mut color = *color0;
                // Bottom half of each block: 50% level of the same color.
                if y >= 4 {
                    for c in color.iter_mut() {
                        *c /= 2;
                    }
                }
                px.extend_from_slice(&color[..]);
            }
        }
    }
    px
}

/// Write the source fixture JPEG for one profile, with the profile embedded
/// in APP2 via the production tagger, and return its path.
fn source_fixture(kind: &str) -> PathBuf {
    let dir = fixture_dir();
    std::fs::create_dir_all(&dir).expect("fixture dir");
    let path = dir.join(format!("src-{kind}.jpg"));
    if !path.exists() {
        let px = fixture_pixels();
        let img = image::RgbImage::from_raw(64, 8, px).expect("fixture buffer");
        let mut jpeg: Vec<u8> = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
            .write_image(img.as_raw(), 64, 8, image::ExtendedColorType::Rgb8)
            .expect("encode fixture");
        fotobase_render::icc::attach_to_jpeg(&mut jpeg, &source_profile(kind));
        std::fs::write(&path, jpeg).expect("write fixture");
    }
    path
}

/// Transformed output for one (source, display) pair, decoded back through
/// the production path: extract the embedded ICC, decode the JPEG, transform.
fn transformed_pixels(src_kind: &str, dst_kind: &str) -> Vec<u8> {
    let path = source_fixture(src_kind);
    let bytes = std::fs::read(&path).expect("read fixture");
    let icc = extract_icc(&bytes).expect("fixture must carry its profile");
    let img = image::load_from_memory(&bytes).expect("decode fixture");
    let rgb = img.to_rgb8().into_raw();
    let mut out = rgb;
    transform_rgb(&mut out, Some(&icc), &display_profile(dst_kind))
        .expect("transform must succeed");
    out
}

/// RMS of per-channel deltas, 0–255 scale.
fn rms(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len(), "pixel count mismatch");
    let sum: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = i64::from(*x) - i64::from(*y);
            d * d
        })
        .sum::<i64>() as f64;
    (sum / a.len() as f64).sqrt()
}

fn write_png(path: &Path, px: &[u8]) {
    let img = image::RgbImage::from_raw(64, 8, px.to_vec()).expect("golden buffer");
    let file = std::fs::File::create(path).expect("create golden file");
    image::codecs::png::PngEncoder::new(std::io::BufWriter::new(file))
        .write_image(img.as_raw(), 64, 8, image::ExtendedColorType::Rgb8)
        .expect("write golden png");
}

fn read_png(path: &Path) -> Vec<u8> {
    image::open(path).expect("read golden png").to_rgb8().into_raw()
}

const SOURCES: [&str; 3] = ["srgb", "display-p3", "adobe-rgb"];
const DISPLAYS: [&str; 2] = ["srgb", "display-p3"];

#[test]
fn golden_display_transform_matrix() {
    let regenerate = std::env::var("FB_GOLDEN_REGENERATE").is_ok();
    let dir = fixture_dir();
    for src in SOURCES {
        for dst in DISPLAYS {
            let golden = dir.join(format!("{src}-to-{dst}.png"));
            let out = transformed_pixels(src, dst);
            if regenerate {
                write_png(&golden, &out);
                continue;
            }
            assert!(
                golden.exists(),
                "missing golden {golden:?} — regenerate with FB_GOLDEN_REGENERATE=1"
            );
            let expected = read_png(&golden);
            let d = rms(&out, &expected);
            assert!(
                d <= RMS_TOLERANCE,
                "{src}→{dst} diverged from golden: RMS {d:.3} > {RMS_TOLERANCE}"
            );
        }
    }
}

/// Mutation proof: replacing the ICC transform with an identity passthrough
/// must be caught by the golden comparison (RMS far above tolerance).
#[test]
fn identity_transform_is_caught_by_the_golden_test() {
    let bytes = std::fs::read(source_fixture("adobe-rgb")).expect("read fixture");
    let untransformed =
        image::load_from_memory(&bytes).expect("decode fixture").to_rgb8().into_raw();
    let golden = read_png(&fixture_dir().join("adobe-rgb-to-display-p3.png"));
    let d = rms(&untransformed, &golden);
    assert!(
        d > RMS_TOLERANCE,
        "identity passthrough must be detected, but RMS {d:.3} ≤ {RMS_TOLERANCE}"
    );
}
