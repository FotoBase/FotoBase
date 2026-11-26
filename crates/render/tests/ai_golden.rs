//! TASK-10-02-04: golden-image tests per AI editing tool on the synthetic
//! recipe fixture (Testing criteria: "mask → adjustment, erase, denoise").
//!
//! Same harness conventions as `recipe_golden.rs`: committed golden PNGs,
//! `FB_GOLDEN_REGENERATE=1` to regenerate, per-pixel tolerance with a diff
//! PNG on failure. The `ai_erase` and `ai_denoise` goldens are gated on the
//! weights in `target/ai-models/` (same pattern as the runtime tests in
//! `fotobase-ai`) — the manifest/checksum gate runs everywhere, the
//! model-execution leg only where the weights are present. The mask →
//! adjustment golden is pure CPU recipe math and runs everywhere.

use std::path::{Path, PathBuf};

use fotobase_core::apply::OpBackend;
use fotobase_core::buffer::LinearImage;
use fotobase_core::recipe::Recipe;
use image::ImageEncoder;
use serde_json::json;

const FIXTURE_DIR: &str = "tests/fixtures/ai-golden";
const W: u32 = 128;
const H: u32 = 96;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_DIR)
}

/// The same synthetic scene as the recipe-golden fixture (gradient
/// background, saturated primaries, red-eye pupil) so the AI goldens show
/// real structure, not flat noise.
fn fixture_pixel(x: u32, y: u32) -> [f32; 3] {
    let (fx, fy) = (x as f32 / W as f32, y as f32 / H as f32);
    let mut px = [0.15 + 0.5 * (fx + fy) / 2.0; 3];
    let mut patch = |cx: f32, cy: f32, hw: f32, hh: f32, c: [f32; 3]| {
        if (fx - cx).abs() < hw && (fy - cy).abs() < hh {
            px = c;
        }
    };
    patch(0.20, 0.20, 0.15, 0.18, [0.9, 0.05, 0.05]);
    patch(0.50, 0.20, 0.15, 0.18, [0.05, 0.9, 0.05]);
    patch(0.20, 0.50, 0.15, 0.18, [0.05, 0.05, 0.9]);
    patch(0.50, 0.50, 0.15, 0.18, [0.9, 0.9, 0.05]);
    px.map(|v| v.clamp(0.0, 1.0))
}

fn encode_png(img: &LinearImage) -> Vec<u8> {
    let srgb: Vec<u8> =
        img.data.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect();
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&srgb, img.width, img.height, image::ExtendedColorType::Rgb8)
        .expect("encode png");
    out
}

fn decode_png(path: &Path) -> LinearImage {
    let img = image::open(path).expect("decode png").to_rgb8();
    LinearImage {
        width: img.width(),
        height: img.height(),
        data: img.into_raw().iter().map(|b| f32::from(*b) / 255.0).collect(),
    }
}

fn check(name: &str, actual: &LinearImage, tol: f64) {
    let golden = fixture_dir().join(format!("{name}.png"));
    if std::env::var("FB_GOLDEN_REGENERATE").is_ok() {
        std::fs::create_dir_all(fixture_dir()).expect("fixture dir");
        std::fs::write(&golden, encode_png(actual)).expect("write golden");
        return;
    }
    let expected = decode_png(&golden);
    let mut max_delta = 0.0f64;
    let mut worst = (0u32, 0u32, 0usize);
    for y in 0..actual.height {
        for x in 0..actual.width {
            for (c, (&a, &e)) in actual.get(x, y).iter().zip(expected.get(x, y).iter()).enumerate()
            {
                let d = ((a - e).abs() as f64 * 255.0).round();
                if d > max_delta {
                    max_delta = d;
                    worst = (x, y, c);
                }
            }
        }
    }
    assert!(
        max_delta <= tol,
        "{name} diverged from golden: max delta {max_delta:.0} > tol {tol:.0} at {:?}",
        worst
    );
}

fn recipe(v: serde_json::Value) -> Recipe {
    Recipe::parse(&v.to_string()).expect("recipe parses").0
}

/// Tool 1 of 3: an AI-selected (or painted) mask region limits a tuning op.
/// Pure recipe math — runs in CI without any weights.
#[test]
fn golden_mask_region_limited_exposure() {
    let fixture = LinearImage::from_fn(W, H, fixture_pixel);
    let r = recipe(json!({ "spec": "fotobase.recipe", "version": 1, "ops": [{
        "type": "exposure",
        "params": { "ev": 0.8, "feather": 0.03,
                    "mask": [[0.35, 0.15], [0.65, 0.15], [0.65, 0.85], [0.35, 0.85]] }
    }] }));
    let out = fotobase_core::apply::apply_recipe(&fixture, &r);
    check("mask_exposure", &out, 1.0);
}

fn models_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ai-models");
    let lama = dir.join("lama_fp32.onnx");
    let scunet = dir.join("scunet_color_real_psnr.onnx");
    (lama.is_file() && scunet.is_file()).then_some(dir)
}

/// Tool 2 of 3: magic erase — LaMa fills the masked region from its
/// surroundings, through the `OpBackend` seam like `lens_correction`.
#[test]
fn golden_ai_erase() {
    let Some(dir) = models_dir() else {
        eprintln!("skipping: AI models not downloaded into target/ai-models");
        return;
    };
    let backend =
        fotobase_render::ai::AiBackend::from_paths(Some(dir.join("lama_fp32.onnx")), None);
    let fixture = LinearImage::from_fn(W, H, fixture_pixel);
    let r = recipe(json!({ "spec": "fotobase.recipe", "version": 1, "ops": [{
        "type": "ai_erase",
        "params": { "feather": 0.02,
                    "mask": [[0.40, 0.35], [0.60, 0.35], [0.60, 0.65], [0.40, 0.65]] }
    }] }));
    let out = fotobase_core::apply::apply_recipe_with(&fixture, &r, &backend);
    // ONNX Runtime CPU inference is deterministic for a pinned model but not
    // bit-stable across OS builds; the golden pins the shape of the result
    // (region filled from surroundings) with generous per-pixel headroom.
    check("ai_erase", &out, 12.0);
}

/// Tool 3 of 3: AI denoise — SCUNet cleans the buffer, blended by `amount`.
/// The fixture gets deterministic synthetic grain so the golden shows
/// denoising, not a near-no-op pass over a clean image.
#[test]
fn golden_ai_denoise() {
    let Some(dir) = models_dir() else {
        eprintln!("skipping: AI models not downloaded into target/ai-models");
        return;
    };
    let backend = fotobase_render::ai::AiBackend::from_paths(
        None,
        Some(dir.join("scunet_color_real_psnr.onnx")),
    );
    let mut fixture = LinearImage::from_fn(W, H, fixture_pixel);
    let mut state = 42u64;
    for px in fixture.data.iter_mut() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *px = (*px + ((state >> 33) % 41) as f32 / 255.0 - 0.08).clamp(0.0, 1.0);
    }
    let r = recipe(json!({ "spec": "fotobase.recipe", "version": 1, "ops": [{
        "type": "ai_denoise", "params": { "amount": 1.0 }
    }] }));
    let out = fotobase_core::apply::apply_recipe_with(&fixture, &r, &backend);
    check("ai_denoise", &out, 12.0);
}

/// The AI backend must decline gracefully when the models are not
/// installed: the op is skipped (render path falls through to the pure
/// engine), never a hard failure.
#[test]
fn ai_backend_without_models_yields_none() {
    let backend = fotobase_render::ai::AiBackend::from_paths(None, None);
    let img = LinearImage::from_fn(8, 8, |_x, _y| [0.5, 0.5, 0.5]);
    let r = recipe(json!({ "spec": "fotobase.recipe", "version": 1, "ops": [{
        "type": "ai_erase",
        "params": { "mask": [[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]] }
    }] }));
    assert!(backend.apply(&img, &r.ops[0]).is_none(), "missing model must decline, not fail");
}

/// Strength 0 `ai_denoise` is a bit-identical no-op (TASK-04-009 semantics).
#[test]
fn ai_denoise_amount_zero_is_a_noop() {
    let backend = fotobase_render::ai::AiBackend::from_paths(None, None);
    let img = LinearImage::from_fn(8, 8, |_x, _y| [0.5, 0.5, 0.5]);
    let r = recipe(json!({ "spec": "fotobase.recipe", "version": 1, "ops": [{
        "type": "ai_denoise", "params": { "amount": 0.0 }
    }] }));
    let out = backend.apply(&img, &r.ops[0]).expect("amount 0 short-circuits before the model");
    assert_eq!(out.data, img.data);
}
