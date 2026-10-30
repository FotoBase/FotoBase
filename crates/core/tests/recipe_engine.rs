//! TASK-04-001: recipe engine core (docs/edit-recipe-spec-v1.md, PLAN §5, §3
//! rules 1 & 5). Tests the versioned op model, schema conformance, unknown-op
//! skip semantics, serialization round-trip, and preview/export resolution
//! parity of the pure-function op implementations.

use std::path::{Path, PathBuf};

use fotobase_core::apply::apply_recipe;
use fotobase_core::buffer::LinearImage;
use fotobase_core::recipe::Recipe;
use serde_json::{json, Value};

fn docs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Gradient test image: every channel varies smoothly so per-pixel ops and
/// geometry ops both produce non-trivial output.
fn gradient(w: u32, h: u32) -> LinearImage {
    LinearImage::from_fn(w, h, |x, y| {
        // Ratios of powers of two so the normalized value is bit-identical
        // across resolutions (1/64 == 4/256 exactly in f32).
        [
            0.25 + 0.5 * x as f32 / w as f32,
            0.25 + 0.5 * y as f32 / h as f32,
            0.25 + 0.5 * (x as f32 / w as f32) * (y as f32 / h as f32),
        ]
    })
}

/// Compare a small render against a 4× render at aligned sample points
/// (small pixel (x, y) corresponds to big pixel (4x, 4y)), so point ops can
/// be compared with no resampling error at all.
fn diff_aligned(small: &LinearImage, big: &LinearImage, scale: u32) -> (f32, f32) {
    let mut sum = 0.0f64;
    let mut max = 0.0f32;
    let mut n = 0usize;
    for y in 0..small.height {
        for x in 0..small.width {
            let sp = small.get(x, y);
            let bp = big.get((x * scale).min(big.width - 1), (y * scale).min(big.height - 1));
            for c in 0..3 {
                let d = (sp[c] - bp[c]).abs();
                sum += d as f64;
                max = max.max(d);
                n += 1;
            }
        }
    }
    ((sum / n as f64) as f32, max)
}

fn recipe_from_ops(ops: Value) -> Recipe {
    let doc = json!({ "spec": "fotobase.recipe", "version": 1, "ops": ops });
    Recipe::parse(&serde_json::to_string(&doc).unwrap()).unwrap().0
}

// ---------------------------------------------------------------------------
// Schema conformance + parsing
// ---------------------------------------------------------------------------

#[test]
fn examples_validate_against_schema_and_parse() {
    let schema = read_json(&docs().join("recipe-v1.schema.json"));
    let validator = jsonschema::validator_for(&schema).unwrap();
    let examples = read_json(&docs().join("recipe-v1.examples.json"));
    let examples = examples.as_array().expect("examples is an array");
    assert!(!examples.is_empty());
    for ex in examples {
        let doc = &ex["recipe"];
        assert!(doc.is_object(), "each example wraps a recipe");
        let errors: Vec<String> = validator.iter_errors(doc).map(|e| e.to_string()).collect();
        assert!(errors.is_empty(), "schema errors for {}: {errors:#?}", ex["name"]);
        Recipe::parse(&serde_json::to_string(doc).unwrap())
            .unwrap_or_else(|e| panic!("parse failed for {}: {e}", ex["name"]));
    }
}

#[test]
fn serialized_recipes_validate_against_schema() {
    let validator =
        jsonschema::validator_for(&read_json(&docs().join("recipe-v1.schema.json"))).unwrap();
    let recipe = recipe_from_ops(json!([
        { "type": "crop", "params": { "x": 0.1, "y": 0.1, "w": 0.5, "h": 0.5 } },
        { "type": "sepia" },
        { "type": "temperature", "params": { "kelvin_shift": 0.4 } }
    ]));
    let doc: Value = serde_json::from_str(&recipe.to_json()).unwrap();
    let errors: Vec<String> = validator.iter_errors(&doc).map(|e| e.to_string()).collect();
    assert!(errors.is_empty(), "own output must be schema-valid: {errors:#?}");
}

#[test]
fn invalid_packets_are_rejected() {
    let cases = [
        json!({ "spec": "other.format", "version": 1, "ops": [] }),
        json!({ "spec": "fotobase.recipe", "version": 0, "ops": [] }),
        // a future spec version cannot be migrated down
        json!({ "spec": "fotobase.recipe", "version": 99, "ops": [] }),
        json!({ "spec": "fotobase.recipe", "version": 1, "ops": "nope" }),
        json!({ "spec": "fotobase.recipe", "version": 1, "ops": [{ "params": {} }] }),
        // known op type with out-of-range params
        json!({ "spec": "fotobase.recipe", "version": 1,
                "ops": [{ "type": "fill_light", "params": { "amount": 1.5 } }] }),
        json!({ "spec": "fotobase.recipe", "version": 1,
                "ops": [{ "type": "crop", "params": { "x": -0.1, "y": 0.0, "w": 0.5, "h": 0.5 } }] }),
        json!({ "spec": "fotobase.recipe", "version": 1,
                "ops": [{ "type": "crop", "params": { "x": 0.0, "y": 0.0, "w": 0.0, "h": 0.5 } }] }),
    ];
    for case in cases {
        let err = Recipe::parse(&serde_json::to_string(&case).unwrap()).unwrap_err();
        assert!(!err.to_string().is_empty(), "case rejected: {case}");
    }
}

#[test]
fn unknown_ops_are_skipped_with_a_warning_not_rejected() {
    let doc = json!({
        "spec": "fotobase.recipe", "version": 1,
        "ops": [
            { "type": "grayscale", "params": { "enabled": true } },
            { "type": "hsl_shift", "params": { "hue": 0.5 } }
        ]
    });
    let (recipe, warnings) = Recipe::parse(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert!(warnings.iter().any(|w| w.contains("hsl_shift")));
    // apply must skip the unknown op, not fail
    let img = gradient(8, 8);
    let out = apply_recipe(&img, &recipe);
    for i in 0..out.data.len() {
        assert_eq!(out.data[i], out.data[i]); // finite
    }
    let g = apply_recipe(&img, &recipe_from_ops(json!([{ "type": "grayscale" }])));
    for i in 0..g.data.len() {
        assert_eq!(out.data[i], g.data[i]);
    }
}

#[test]
fn missing_params_use_documented_defaults() {
    let img = gradient(16, 16);
    let explicit = recipe_from_ops(json!([{ "type": "fill_light", "params": { "amount": 0.5 } }]));
    let omitted = recipe_from_ops(json!([{ "type": "fill_light" }]));
    let omitted_empty = recipe_from_ops(json!([{ "type": "fill_light", "params": {} }]));
    let a = apply_recipe(&img, &explicit);
    let b = apply_recipe(&img, &omitted);
    let c = apply_recipe(&img, &omitted_empty);
    assert_eq!(a.data, b.data);
    assert_eq!(a.data, c.data);
}

// ---------------------------------------------------------------------------
// Round-trip
// ---------------------------------------------------------------------------

#[test]
fn serialization_round_trips_identical_ops() {
    let recipe = recipe_from_ops(json!([
        { "type": "crop", "params": { "x": 0.1, "y": 0.2, "w": 0.7, "h": 0.6 } },
        { "type": "straighten", "params": { "angle": 3.5, "auto_fill": true } },
        { "type": "auto_color" },
        { "type": "auto_contrast" },
        { "type": "fill_light", "params": { "amount": 0.3 } },
        { "type": "temperature", "params": { "kelvin_shift": -0.4 } },
        { "type": "tint", "params": { "green_magenta": 0.2 } },
        { "type": "saturation", "params": { "amount": 0.5 } },
        { "type": "brightness", "params": { "amount": -0.2 } },
        { "type": "highlight_recovery", "params": { "amount": 0.6 } },
        { "type": "shadow_recovery", "params": { "amount": 0.4 } },
        { "type": "sepia", "params": { "amount": 0.8 } },
        { "type": "warm_colors", "params": { "amount": 0.5 } },
        { "type": "cool_colors", "params": { "amount": 0.25 } },
        { "type": "red_eye", "params": { "regions": [{ "x": 0.3, "y": 0.3, "r": 0.05 }] } },
        { "type": "text", "params": { "text": "FOTOBASE 2026", "x": 0.1, "y": 0.8,
            "size": 0.1, "color": [1.0, 1.0, 1.0], "font": "sans", "rotation": 0 } },
        { "type": "retouch", "params": { "strokes": [{ "points": [[0.5, 0.5], [0.6, 0.5]], "radius": 0.05 }] } },
        { "type": "grayscale", "params": { "enabled": true } },
        { "type": "sharpen", "params": { "amount": 0.5 } },
        { "type": "undo_all" }
    ]));
    let round = Recipe::parse(&recipe.to_json()).unwrap().0;
    let a: Value = serde_json::from_str(&recipe.to_json()).unwrap();
    let b: Value = serde_json::from_str(&round.to_json()).unwrap();
    assert_eq!(a, b, "serialize → parse must yield the identical op list");
}

// ---------------------------------------------------------------------------
// Op semantics
// ---------------------------------------------------------------------------

#[test]
fn crop_selects_the_normalized_rect() {
    let img = gradient(10, 10);
    let recipe = recipe_from_ops(
        json!([{ "type": "crop", "params": { "x": 0.2, "y": 0.1, "w": 0.5, "h": 0.4 } }]),
    );
    let out = apply_recipe(&img, &recipe);
    assert_eq!((out.width, out.height), (5, 4));
    // top-left of the crop equals pixel (2, 1) of the source
    assert_eq!(out.get(0, 0), img.get(2, 1));
}

#[test]
fn undo_all_restores_the_original() {
    let img = gradient(16, 16);
    let recipe = recipe_from_ops(json!([
        { "type": "sepia", "params": { "amount": 1.0 } },
        { "type": "crop", "params": { "x": 0.0, "y": 0.0, "w": 0.5, "h": 0.5 } },
        { "type": "undo_all" }
    ]));
    let out = apply_recipe(&img, &recipe);
    assert_eq!((out.width, out.height), (16, 16));
    assert_eq!(out.data, img.data);
}

#[test]
fn basic_ops_behave() {
    let img = gradient(8, 8);
    // grayscale collapses to a single luminance value per pixel
    let g = apply_recipe(&img, &recipe_from_ops(json!([{ "type": "grayscale" }])));
    for p in 0..(g.width * g.height) as usize {
        let [r, gr, b] = g.get((p % 8) as u32, (p / 8) as u32);
        assert!((r - gr).abs() < 1e-6 && (gr - b).abs() < 1e-6);
    }
    // brightness -1 → black
    let b = apply_recipe(
        &img,
        &recipe_from_ops(json!([{ "type": "brightness", "params": { "amount": -1.0 } }])),
    );
    assert!(b.data.iter().all(|&v| v <= 1e-6));
    // temperature warms: red up, blue down
    let w = apply_recipe(
        &img,
        &recipe_from_ops(json!([{ "type": "temperature", "params": { "kelvin_shift": 1.0 } }])),
    );
    assert!(w.data[0] > img.data[0]);
    assert!(w.data[2] < img.data[2]);
}

// ---------------------------------------------------------------------------
// Resolution parity (preview vs export — same op code, PLAN §3 rule 5)
// ---------------------------------------------------------------------------

#[test]
fn point_ops_are_resolution_independent() {
    let recipe = recipe_from_ops(json!([
        { "type": "temperature", "params": { "kelvin_shift": 0.3 } },
        { "type": "fill_light", "params": { "amount": 0.4 } },
        { "type": "saturation", "params": { "amount": 0.5 } },
        { "type": "sepia", "params": { "amount": 0.7 } },
        { "type": "shadow_recovery", "params": { "amount": 0.5 } },
        { "type": "highlight_recovery", "params": { "amount": 0.5 } }
    ]));
    let small = apply_recipe(&gradient(64, 48), &recipe);
    let big = apply_recipe(&gradient(256, 192), &recipe);
    let (mean, max) = diff_aligned(&small, &big, 4);
    assert!(max <= 2e-3, "point ops must match across resolutions (mean {mean}, max {max})");
}

#[test]
fn statistical_and_spatial_ops_match_within_tolerance() {
    // auto_color/auto_contrast/sharpen legitimately differ sub-pixel across
    // resolutions (spec §2); golden tolerance per TASK-04-001 acceptance.
    let recipe = recipe_from_ops(json!([
        { "type": "auto_color" },
        { "type": "auto_contrast" },
        { "type": "sharpen", "params": { "amount": 0.5 } }
    ]));
    let small = apply_recipe(&gradient(64, 48), &recipe);
    let big = apply_recipe(&gradient(256, 192), &recipe);
    let (mean, max) = diff_aligned(&small, &big, 4);
    assert!(
        mean <= 0.02 && max <= 0.08,
        "statistical ops within tolerance (mean {mean}, max {max})"
    );
}

#[test]
fn geometric_ops_match_within_tolerance() {
    let recipe = recipe_from_ops(json!([
        { "type": "crop", "params": { "x": 0.1, "y": 0.1, "w": 0.8, "h": 0.8 } },
        { "type": "straighten", "params": { "angle": 5.0, "auto_fill": true } }
    ]));
    let small = apply_recipe(&gradient(64, 48), &recipe);
    let big = apply_recipe(&gradient(256, 192), &recipe);
    let (mean, max) = diff_aligned(&small, &big, 4);
    assert!(mean <= 0.03 && max <= 0.1, "geometry within tolerance (mean {mean}, max {max})");
}
