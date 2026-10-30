//! TASK-04-004: golden-image harness for recipe ops (PLAN §9 Render
//! fidelity, §6 epic acceptance "identical results at preview and export
//! resolution").
//!
//! Every v1 recipe op is rendered on a committed synthetic fixture at
//! preview resolution (128×96) and full resolution (512×384, ×4 bilinear
//! upscale of the same fixture) and compared against committed golden PNGs
//! with a per-op per-pixel tolerance. On failure a per-pixel diff PNG is
//! written and the worst pixel is reported — not just pass/fail.
//!
//! Regenerate goldens with
//! `FB_GOLDEN_REGENERATE=1 cargo test -p fotobase-render --test recipe_golden`
//! (then review the diff before committing).

use std::path::{Path, PathBuf};

use fotobase_core::apply::apply_recipe;
use fotobase_core::buffer::LinearImage;
use fotobase_core::recipe::Recipe;
use image::ImageEncoder;
use serde_json::json;

const FIXTURE_DIR: &str = "tests/fixtures/recipe-golden";
const PREVIEW_W: u32 = 128;
const PREVIEW_H: u32 = 96;
const FULL_SCALE: u32 = 4;

/// Per-op tolerance: maximum per-pixel channel delta on the 0–255 scale.
/// Pure per-pixel math is exactly reproducible (f64 → f32, same code on all
/// OSes), so 1.0 is generous; geometry ops (bilinear resampling in
/// crop/straighten) and neighborhood ops (sharpen, retouch) get 2.0 for
/// rounding headroom.
const TOLERANCES: &[(&str, f64)] = &[
    ("auto_color", 1.0),
    ("auto_contrast", 1.0),
    ("brightness", 1.0),
    ("cool_colors", 1.0),
    ("crop", 2.0),
    ("fill_light", 1.0),
    ("grayscale", 1.0),
    ("highlight_recovery", 1.0),
    ("red_eye", 1.0),
    ("retouch", 2.0),
    ("saturation", 1.0),
    ("sepia", 1.0),
    ("shadow_recovery", 1.0),
    ("sharpen", 2.0),
    ("straighten", 2.0),
    ("temperature", 1.0),
    ("text", 1.0),
    ("tint", 1.0),
    // PNG goldens are quantized to 8 bit, so even a bit-exact op shows
    // ±1 on the 0–255 scale after a decode/compare round trip.
    ("undo_all", 1.0),
    ("warm_colors", 1.0),
];

fn fixture_dir() -> PathBuf {
    // Cargo sets CWD to the crate root for integration tests.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_DIR)
}

/// Deterministic synthetic scene exercising every op's input domain:
/// gradient background, saturated primaries, a skin patch with a red-eye
/// pupil, a dark patch for text, and a uniform patch for retouch. All
/// values are linear-light [0, 1] — the recipe engine's native domain.
fn fixture_pixel(x: u32, y: u32) -> [f32; 3] {
    let w = PREVIEW_W as f32;
    let h = PREVIEW_H as f32;
    let (fx, fy) = (x as f32 / w, y as f32 / h);
    // Diagonal gray gradient background.
    let mut px = [0.15 + 0.5 * (fx + fy) / 2.0; 3];
    // 2×2 saturated primaries at the top-left.
    let mut patch = |cx: f32, cy: f32, hw: f32, hh: f32, c: [f32; 3]| {
        if (fx - cx).abs() < hw && (fy - cy).abs() < hh {
            px = c;
        }
    };
    patch(0.20, 0.20, 0.15, 0.18, [0.9, 0.05, 0.05]);
    patch(0.50, 0.20, 0.15, 0.18, [0.05, 0.9, 0.05]);
    patch(0.20, 0.50, 0.15, 0.18, [0.05, 0.05, 0.9]);
    patch(0.50, 0.50, 0.15, 0.18, [0.9, 0.9, 0.05]);
    // Ellipse with horizontal radius `r` in fx units (matches the red_eye
    // op's rx = r*width, ry = r*height geometry).
    let ell = |cx: f32, cy: f32, r: f32| {
        let dx = fx - cx;
        let dy = (fy - cy) * w / h;
        dx * dx + dy * dy < r * r
    };
    // Skin patch with a red-eye pupil at a known normalized location
    // (0.75, 0.30) — the red_eye golden targets exactly this pupil. The
    // skin tone must NOT trip the op's red-dominance test (r > 1.3·g and
    // r > 1.3·b), only the pupil may.
    if ell(0.75, 0.30, 0.05) {
        px = [0.75, 0.62, 0.50];
    }
    if ell(0.75, 0.30, 0.02) {
        px = [0.9, 0.04, 0.04];
    }
    // Dark patch (bottom-right) for legible text.
    if fx > 0.80 && fy > 0.80 {
        px = [0.03, 0.03, 0.03];
    }
    px.map(|v| v.clamp(0.0, 1.0))
}

fn fixture_bytes() -> Vec<u8> {
    let img = LinearImage::from_fn(PREVIEW_W, PREVIEW_H, fixture_pixel);
    encode_png(&img)
}

/// Load the committed fixture (or write it when regenerating).
fn load_fixture() -> LinearImage {
    let path = fixture_dir().join("fixture.png");
    if std::env::var("FB_GOLDEN_REGENERATE").is_ok() {
        std::fs::write(&path, fixture_bytes()).expect("write fixture");
    }
    assert!(path.exists(), "missing fixture {path:?} — regenerate with FB_GOLDEN_REGENERATE=1");
    decode_png(&path)
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
    let (w, h) = (img.width(), img.height());
    LinearImage {
        width: w,
        height: h,
        data: img.into_raw().iter().map(|b| f32::from(*b) / 255.0).collect(),
    }
}

/// ×N bilinear upscale of the fixture — stands in for the full-resolution
/// source so both resolutions exercise the same scene content.
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

/// One golden case: op name, its recipe JSON, and the per-op tolerance.
struct GoldenCase {
    name: &'static str,
    recipe: serde_json::Value,
    tol: f64,
}

fn cases() -> Vec<GoldenCase> {
    let by_op = |name: &str| TOLERANCES.iter().find(|(n, _)| *n == name).unwrap().1;
    let op = |name: &str, params: serde_json::Value| json!({ "spec": "fotobase.recipe", "version": 1, "ops": [{ "type": name, "params": params }] });
    let mut cases = vec![
        GoldenCase {
            name: "auto_color",
            recipe: op("auto_color", json!({})),
            tol: by_op("auto_color"),
        },
        GoldenCase {
            name: "auto_contrast",
            recipe: op("auto_contrast", json!({})),
            tol: by_op("auto_contrast"),
        },
        GoldenCase {
            name: "brightness",
            recipe: op("brightness", json!({ "amount": 0.3 })),
            tol: by_op("brightness"),
        },
        GoldenCase {
            name: "cool_colors",
            recipe: op("cool_colors", json!({ "amount": 0.5 })),
            tol: by_op("cool_colors"),
        },
        GoldenCase {
            name: "crop",
            recipe: op("crop", json!({ "x": 0.1, "y": 0.1, "w": 0.6, "h": 0.6 })),
            tol: by_op("crop"),
        },
        GoldenCase {
            name: "fill_light",
            recipe: op("fill_light", json!({ "amount": 0.5 })),
            tol: by_op("fill_light"),
        },
        GoldenCase {
            name: "grayscale",
            recipe: op("grayscale", json!({ "enabled": true })),
            tol: by_op("grayscale"),
        },
        GoldenCase {
            name: "highlight_recovery",
            recipe: op("highlight_recovery", json!({ "amount": 0.5 })),
            tol: by_op("highlight_recovery"),
        },
        GoldenCase {
            name: "red_eye",
            recipe: op("red_eye", json!({ "regions": [{ "x": 0.75, "y": 0.30, "r": 0.03 }] })),
            tol: by_op("red_eye"),
        },
        // Retouch targets the gradient background (no uniform patch — the
        // op must visibly blend the stroked area against surrounding
        // content, otherwise the golden is vacuous).
        GoldenCase {
            name: "retouch",
            recipe: op(
                "retouch",
                json!({ "strokes": [{ "points": [[0.5, 0.85]], "radius": 0.05 }] }),
            ),
            tol: by_op("retouch"),
        },
        GoldenCase {
            name: "saturation",
            recipe: op("saturation", json!({ "amount": 0.5 })),
            tol: by_op("saturation"),
        },
        GoldenCase {
            name: "sepia",
            recipe: op("sepia", json!({ "amount": 1.0 })),
            tol: by_op("sepia"),
        },
        GoldenCase {
            name: "shadow_recovery",
            recipe: op("shadow_recovery", json!({ "amount": 0.5 })),
            tol: by_op("shadow_recovery"),
        },
        GoldenCase {
            name: "sharpen",
            recipe: op("sharpen", json!({ "amount": 0.5 })),
            tol: by_op("sharpen"),
        },
        GoldenCase {
            name: "straighten",
            recipe: op("straighten", json!({ "angle": 5.0, "auto_fill": true })),
            tol: by_op("straighten"),
        },
        GoldenCase {
            name: "temperature",
            recipe: op("temperature", json!({ "kelvin_shift": 0.4 })),
            tol: by_op("temperature"),
        },
        GoldenCase {
            name: "text",
            recipe: op(
                "text",
                json!({ "text": "FB", "x": 0.02, "y": 0.02, "size": 0.08, "color": "#ffffff" }),
            ),
            tol: by_op("text"),
        },
        GoldenCase {
            name: "tint",
            recipe: op("tint", json!({ "green_magenta": -0.3 })),
            tol: by_op("tint"),
        },
        GoldenCase {
            name: "warm_colors",
            recipe: op("warm_colors", json!({ "amount": 0.5 })),
            tol: by_op("warm_colors"),
        },
        // `undo_all` restarts from the original: [fill_light, undo_all] must
        // round-trip to the untouched fixture, bit-exact.
        GoldenCase {
            name: "undo_all",
            recipe: json!({ "spec": "fotobase.recipe", "version": 1, "ops": [
                { "type": "fill_light", "params": { "amount": 0.8 } },
                { "type": "undo_all", "params": {} },
            ] }),
            tol: by_op("undo_all"),
        },
    ];
    cases.sort_by_key(|c| c.name);
    cases
}

/// Per-pixel comparison result.
struct Report {
    max_delta: f64,
    rms: f64,
    worst: Option<(u32, u32, usize)>,
}

fn compare(actual: &LinearImage, expected: &LinearImage) -> Report {
    assert_eq!(actual.width, expected.width, "width mismatch");
    assert_eq!(actual.height, expected.height, "height mismatch");
    let mut max_delta = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut worst = None;
    for y in 0..actual.height {
        for x in 0..actual.width {
            let (a, e) = (actual.get(x, y), expected.get(x, y));
            for (c, (&av, &ev)) in a.iter().zip(e.iter()).enumerate() {
                let d = ((av - ev).abs() as f64 * 255.0).round();
                sum_sq += d * d;
                if d > max_delta {
                    max_delta = d;
                    worst = Some((x, y, c));
                }
            }
        }
    }
    let n = f64::from(actual.width * actual.height * 3);
    Report { max_delta, rms: (sum_sq / n).sqrt(), worst }
}

/// Write an amplified (×32) per-pixel diff PNG so failures show *where* the
/// output diverged, not just that it did.
fn write_diff_png(name: &str, actual: &LinearImage, expected: &LinearImage) -> PathBuf {
    let mut diff = LinearImage::new(actual.width, actual.height);
    for y in 0..actual.height {
        for x in 0..actual.width {
            let (a, e) = (actual.get(x, y), expected.get(x, y));
            let d: [f32; 3] = std::array::from_fn(|c| ((a[c] - e[c]).abs() * 32.0).clamp(0.0, 1.0));
            diff.set(x, y, d);
        }
    }
    let dir = std::env::temp_dir().join("recipe-golden-diffs");
    std::fs::create_dir_all(&dir).expect("diff dir");
    let path = dir.join(format!("{name}.diff.png"));
    std::fs::write(&path, encode_png(&diff)).expect("write diff png");
    path
}

fn check_case(case: &GoldenCase, input: &LinearImage, label: &str) -> Result<(), String> {
    let (recipe, _) = Recipe::parse(&case.recipe.to_string())
        .unwrap_or_else(|e| panic!("{} recipe must parse: {e}", case.name));
    let actual = apply_recipe(input, &recipe);
    let golden = fixture_dir().join(format!("{}-{label}.png", case.name));
    if std::env::var("FB_GOLDEN_REGENERATE").is_ok() {
        std::fs::write(&golden, encode_png(&actual)).expect("write golden");
        return Ok(());
    }
    assert!(golden.exists(), "missing golden {golden:?} — regenerate with FB_GOLDEN_REGENERATE=1");
    let expected = decode_png(&golden);
    let report = compare(&actual, &expected);
    if report.max_delta <= case.tol {
        return Ok(());
    }
    let (x, y, c) = report.worst.expect("max_delta > 0 must have a worst pixel");
    let diff_path = write_diff_png(&format!("{}-{label}", case.name), &actual, &expected);
    Err(format!(
        "{} [{label}] diverged from golden: max per-pixel delta {:.0} > tol {:.0} \
         (RMS {:.3}) at pixel ({x}, {y}, channel {c}); diff written to {diff_path:?}",
        case.name, report.max_delta, case.tol, report.rms
    ))
}

/// Integration: every op's preview-res and full-res outputs match their
/// goldens within the per-op tolerance (Testing criteria).
#[test]
fn every_recipe_op_matches_its_goldens() {
    let preview = load_fixture();
    let full = upscale(&preview, FULL_SCALE);
    for case in cases() {
        check_case(&case, &preview, "preview").unwrap_or_else(|e| panic!("{e}"));
        check_case(&case, &full, "full").unwrap_or_else(|e| panic!("{e}"));
    }
}

/// Meta-test: an intentionally perturbed op output must be detected as a
/// failure by the harness (Testing criteria).
#[test]
fn harness_detects_a_perturbed_output() {
    let preview = load_fixture();
    let (recipe, _) = Recipe::parse(
        &json!({ "spec": "fotobase.recipe", "version": 1,
                 "ops": [{ "type": "grayscale", "params": { "enabled": true } }] })
        .to_string(),
    )
    .expect("recipe parses");
    let mut perturbed = apply_recipe(&preview, &recipe);
    let (x, y) = (preview.width / 2, preview.height / 2);
    let mut px = perturbed.get(x, y);
    px[0] += 5.0 / 255.0;
    perturbed.set(x, y, px);
    let case_tol = TOLERANCES.iter().find(|(n, _)| *n == "grayscale").unwrap().1;
    let golden = decode_png(&fixture_dir().join("grayscale-preview.png"));
    let report = compare(&perturbed, &golden);
    assert!(
        report.max_delta > case_tol,
        "perturbation must exceed tolerance, but max delta {:.0} ≤ {case_tol}",
        report.max_delta
    );
    let (x, y, c) = report.worst.expect("max_delta > 0 must have a worst pixel");
    assert_eq!((x, y, c), (preview.width / 2, preview.height / 2, 0));
}

/// undo_all must round-trip to the original bit-exactly even at full res.
#[test]
fn undo_all_is_lossless() {
    let preview = load_fixture();
    let full = upscale(&preview, FULL_SCALE);
    for input in [&preview, &full] {
        let (recipe, _) = Recipe::parse(
            &json!({ "spec": "fotobase.recipe", "version": 1, "ops": [
                { "type": "fill_light", "params": { "amount": 0.8 } },
                { "type": "undo_all", "params": {} },
            ] })
            .to_string(),
        )
        .expect("recipe parses");
        let out = apply_recipe(input, &recipe);
        assert_eq!(out, *input, "undo_all must restore the original exactly");
    }
}
