#![cfg(feature = "gpu")]

//! TASK-10-02-03: the wgpu GPU path must match the CPU recipe engine within
//! the documented per-op tolerance (docs/edit-recipe-spec-v1.md §2; the CPU
//! path stays the golden reference). Skips cleanly when no GPU adapter is
//! present (headless CI without a software rasterizer).

use fotobase_core::apply::apply_recipe;
use fotobase_core::buffer::LinearImage;
use fotobase_core::recipe::Recipe;
use fotobase_render::gpu::GpuBackend;
use serde_json::json;

const PREVIEW_W: u32 = 128;
const PREVIEW_H: u32 = 96;
const FULL_SCALE: u32 = 4;

/// Same synthetic scene as `recipe_golden.rs`, so the GPU and the goldens
/// exercise identical content.
fn fixture_pixel(x: u32, y: u32) -> [f32; 3] {
    let w = PREVIEW_W as f32;
    let h = PREVIEW_H as f32;
    let (fx, fy) = (x as f32 / w, y as f32 / h);
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

fn fixture() -> LinearImage {
    LinearImage::from_fn(PREVIEW_W, PREVIEW_H, fixture_pixel)
}

/// ×N bilinear upscale, copied from `recipe_golden.rs`.
fn upscale(img: &LinearImage, scale: u32) -> LinearImage {
    let out_w = img.width * scale;
    let out_h = img.height * scale;
    LinearImage::from_fn(out_w, out_h, |x, y| {
        let fx = (x as f32 + 0.5) / scale as f32 - 0.5;
        let fy = (y as f32 + 0.5) / scale as f32 - 0.5;
        let x0 = fx.floor().max(0.0) as i64;
        let y0 = fy.floor().max(0.0) as i64;
        let x1 = (x0 + 1).min(i64::from(img.width) - 1);
        let y1 = (y0 + 1).min(i64::from(img.height) - 1);
        let tx = (fx - x0 as f32).clamp(0.0, 1.0);
        let ty = (fy - y0 as f32).clamp(0.0, 1.0);
        let a = img.get(x0 as u32, y0 as u32);
        let b = img.get(x1 as u32, y0 as u32);
        let c = img.get(x0 as u32, y1 as u32);
        let d = img.get(x1 as u32, y1 as u32);
        let mix = |p: [f32; 3], q: [f32; 3], t: f32| {
            [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t, p[2] + (q[2] - p[2]) * t]
        };
        mix(mix(a, b, tx), mix(c, d, tx), ty)
    })
}

fn op(name: &str, params: serde_json::Value) -> Recipe {
    serde_json::from_value(json!({
        "spec": "fotobase.recipe", "version": 1,
        "ops": [{ "type": name, "params": params }]
    }))
    .expect("valid recipe")
}

/// Per-op GPU-vs-CPU tolerance on the 0–255 scale. The GPU shader is f32
/// while some CPU ops (curves) compute in f64, and the CPU reference here is
/// the plain `fotobase_core` engine (no Lensfun); 1.0 matches the golden
/// harness's per-pixel-op tolerance.
const GPU_TOLERANCES: &[(&str, f64)] = &[
    ("auto_rotate", 2.0), // CPU-only op: covered via the mixed recipe
    ("bw", 1.0),
    ("brightness", 1.0),
    ("cool_colors", 1.0),
    ("curves", 1.0),
    ("exposure", 1.0),
    ("fill_light", 1.0),
    ("grayscale", 1.0),
    ("highlight_recovery", 1.0),
    ("hsl", 1.0),
    ("keystone", 2.0), // CPU-only op: covered via the mixed recipe
    ("levels", 1.0),
    ("saturation", 1.0),
    ("sepia", 1.0),
    ("shadow_recovery", 1.0),
    ("temperature", 1.0),
    ("tint", 1.0),
    ("tint_color", 1.0),
    ("vibrance", 1.0),
    ("vignette", 1.0),
    ("warm_colors", 1.0),
    ("warmify", 1.0),
];

fn cases() -> Vec<(&'static str, Recipe)> {
    vec![
        ("bw", op("bw", json!({ "amount": 0.8 }))),
        ("brightness", op("brightness", json!({ "amount": 0.3 }))),
        ("cool_colors", op("cool_colors", json!({ "amount": 0.6 }))),
        ("curves", op("curves", json!({ "points": [[0.0, 0.05], [0.5, 0.6], [1.0, 0.95]] }))),
        ("exposure", op("exposure", json!({ "ev": 0.7 }))),
        ("fill_light", op("fill_light", json!({ "amount": 0.5 }))),
        ("grayscale", op("grayscale", json!({}))),
        ("highlight_recovery", op("highlight_recovery", json!({ "amount": 0.7 }))),
        ("hsl", op("hsl", json!({ "hue": 0.1, "saturation": 0.4, "luminance": 0.05 }))),
        ("levels", op("levels", json!({ "black": 0.1, "white": 0.9, "gamma": 1.2 }))),
        ("saturation", op("saturation", json!({ "amount": 0.5 }))),
        ("sepia", op("sepia", json!({ "amount": 0.8 }))),
        ("shadow_recovery", op("shadow_recovery", json!({ "amount": 0.7 }))),
        ("temperature", op("temperature", json!({ "kelvin_shift": -0.4 }))),
        ("tint", op("tint", json!({ "green_magenta": 0.3 }))),
        ("tint_color", op("tint", json!({ "color": "#ff8000", "amount": 0.6 }))),
        ("vibrance", op("vibrance", json!({ "amount": 0.6 }))),
        ("vignette", op("vignette", json!({ "amount": 0.7 }))),
        ("warm_colors", op("warm_colors", json!({ "amount": 0.6 }))),
        ("warmify", op("warmify", json!({ "amount": 0.5 }))),
    ]
}

fn assert_close(cpu: &LinearImage, gpu: &LinearImage, tol: f64, label: &str) {
    assert_eq!(cpu.width, gpu.width, "{label}: width");
    assert_eq!(cpu.height, gpu.height, "{label}: height");
    let mut worst = 0.0f64;
    let mut where_ = (0usize, 0usize);
    for (i, (c, g)) in cpu.data.iter().zip(&gpu.data).enumerate() {
        let d = (c - g).abs() as f64 * 255.0;
        if d > worst {
            worst = d;
            where_ = (i / 3, (i / 3) % cpu.width as usize);
        }
    }
    assert!(worst <= tol, "{label}: worst per-pixel delta {worst:.2} > {tol} at px {:?}", where_);
}

#[test]
fn gpu_matches_cpu_per_op() {
    let backend = match GpuBackend::new() {
        Some(b) => b,
        None => {
            eprintln!("no wgpu adapter — skipping GPU/CPU comparison");
            return;
        }
    };
    let base = fixture();
    let full = upscale(&base, FULL_SCALE);
    for (name, recipe) in cases() {
        let tol = GPU_TOLERANCES.iter().find(|(n, _)| *n == name).unwrap().1;
        let cpu = apply_recipe(&base, &recipe);
        let gpu = backend.apply(&base, &recipe.ops[0]).expect("GPU backend handles case");
        assert_close(&cpu, &gpu, tol, &format!("{name} @ preview"));
        let cpu_full = apply_recipe(&full, &recipe);
        let gpu_full = backend.apply(&full, &recipe.ops[0]).expect("GPU backend handles case");
        assert_close(&cpu_full, &gpu_full, tol, &format!("{name} @ full"));
    }
}

#[test]
fn gpu_apply_recipe_matches_cpu_on_mixed_recipe() {
    let backend = match GpuBackend::new() {
        Some(b) => b,
        None => {
            eprintln!("no wgpu adapter — skipping GPU/CPU comparison");
            return;
        }
    };
    // Mixed recipe: GPU per-pixel ops interleaved with a CPU-only op.
    let recipe: Recipe = serde_json::from_value(json!({
        "spec": "fotobase.recipe", "version": 1,
        "ops": [
            { "type": "exposure", "params": { "ev": 0.4 } },
            { "type": "curves", "params": { "points": [[0.0, 0.0], [0.5, 0.55], [1.0, 1.0]] } },
            { "type": "keystone", "params": { "top": 0.1, "bottom": -0.1 } },
            { "type": "vibrance", "params": { "amount": 0.5 } }
        ]
    }))
    .unwrap();
    let img = upscale(&fixture(), 2);
    let cpu = apply_recipe(&img, &recipe);
    let gpu = fotobase_core::apply::apply_recipe_with(&img, &recipe, &backend);
    assert_close(&cpu, &gpu, 2.0, "mixed recipe");
}

#[test]
fn gpu_backend_rejects_unsupported_ops_with_none() {
    let backend = match GpuBackend::new() {
        Some(b) => b,
        None => {
            eprintln!("no wgpu adapter — skipping GPU/CPU comparison");
            return;
        }
    };
    let img = fixture();
    // crop is a geometry op: the GPU backend must return None so the caller
    // falls back to the CPU path.
    assert!(backend
        .apply(
            &img,
            &op("crop", json!({ "left": 0.1, "top": 0.1, "right": 0.9, "bottom": 0.9 })).ops[0]
        )
        .is_none());
    assert!(backend.apply(&img, &op("lens_correction", json!({})).ops[0]).is_none());
    assert!(backend.apply(&img, &op("unknown_future_op", json!({})).ops[0]).is_none());

    let oversized = LinearImage { width: 16_384, height: 16_384, data: Vec::new() };
    assert!(backend.apply(&oversized, &op("exposure", json!({ "ev": 0.5 })).ops[0]).is_none());
}
