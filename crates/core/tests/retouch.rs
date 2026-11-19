//! TASK-08-002: heal brush as the `retouch` recipe op (PLAN §2 §E Retouch,
//! §5; docs/edit-recipe-spec-v1.md §3). Engine-level behavior; the
//! pixel-exact golden lives in `crates/render/tests/recipe_golden.rs`.

use fotobase_core::apply::apply_recipe;
use fotobase_core::buffer::LinearImage;
use fotobase_core::recipe::Recipe;
use serde_json::{json, Value};

fn recipe(strokes: Value) -> Recipe {
    let doc = json!({
        "spec": "fotobase.recipe", "version": 1,
        "ops": [{ "type": "retouch", "params": { "strokes": strokes } }]
    });
    Recipe::parse(&doc.to_string()).unwrap().0
}

fn run(img: &LinearImage, strokes: Value) -> LinearImage {
    apply_recipe(img, &recipe(strokes))
}

/// Smooth scene sampled at any resolution (continuous in normalized
/// coordinates), so preview and export see the same underlying image.
fn gradient(w: u32, h: u32) -> LinearImage {
    LinearImage::from_fn(w, h, |x, y| {
        let (fx, fy) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
        [0.2 + 0.6 * fx, 0.2 + 0.6 * fy, 0.4 + 0.2 * fx * fy]
    })
}

/// Paint a solid dot of `radius_px` at pixel (cx, cy).
fn blemish(img: &mut LinearImage, cx: f32, cy: f32, radius_px: f32, px: [f32; 3]) {
    for y in 0..img.height {
        for x in 0..img.width {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            if dx * dx + dy * dy <= radius_px * radius_px {
                img.set(x, y, px);
            }
        }
    }
}

fn max_diff(a: &LinearImage, b: &LinearImage, x0: u32, y0: u32, x1: u32, y1: u32) -> f32 {
    let mut worst = 0.0f32;
    for y in y0..y1 {
        for x in x0..x1 {
            let (p, q) = (a.get(x, y), b.get(x, y));
            for c in 0..3 {
                worst = worst.max((p[c] - q[c]).abs());
            }
        }
    }
    worst
}

#[test]
fn heals_a_solid_blemish_back_to_the_gradient() {
    let clean = gradient(200, 150);
    let mut dirty = clean.clone();
    blemish(&mut dirty, 100.0, 75.0, 8.0, [1.0, 0.0, 0.0]);
    // Stroke covers the blemish (radius 0.08 × 150 = 12 px > 8 px).
    let healed = run(&dirty, json!([{ "points": [[0.5, 0.5]], "radius": 0.08 }]));
    let err = max_diff(&healed, &clean, 88, 63, 112, 87);
    assert!(err < 0.01, "healed blemish deviates {err} from the gradient");
    // Sanity: without the heal the blemish is far off.
    assert!(max_diff(&dirty, &clean, 88, 63, 112, 87) > 0.5);
}

#[test]
fn heals_a_scratch_along_a_multi_point_stroke() {
    let clean = gradient(256, 128);
    let mut dirty = clean.clone();
    for x in 40..216 {
        blemish(&mut dirty, x as f32, 64.0, 2.0, [0.0, 0.0, 0.0]);
    }
    let healed =
        run(&dirty, json!([{ "points": [[0.15, 0.5], [0.5, 0.5], [0.85, 0.5]], "radius": 0.04 }]));
    let err = max_diff(&healed, &clean, 30, 50, 226, 78);
    assert!(err < 0.01, "healed scratch deviates {err}");
}

#[test]
fn large_stroke_converges_instead_of_leaving_a_dark_core() {
    let clean = gradient(640, 480);
    let mut dirty = clean.clone();
    blemish(&mut dirty, 320.0, 240.0, 70.0, [0.0, 0.0, 0.0]);
    let healed = run(&dirty, json!([{ "points": [[0.5, 0.5]], "radius": 0.2 }]));
    let err = max_diff(&healed, &clean, 200, 130, 440, 350);
    assert!(err < 0.02, "large heal deviates {err}");
}

#[test]
fn pixels_outside_the_stroke_are_untouched() {
    let mut img = gradient(100, 100);
    blemish(&mut img, 50.0, 50.0, 4.0, [1.0, 1.0, 1.0]);
    let healed = run(&img, json!([{ "points": [[0.5, 0.5]], "radius": 0.1 }]));
    // Stroke radius is 10 px: everything beyond is bit-identical.
    for y in 0..100u32 {
        for x in 0..100u32 {
            let (dx, dy) = (x as f32 + 0.5 - 50.0, y as f32 + 0.5 - 50.0);
            if dx * dx + dy * dy > 10.5 * 10.5 {
                assert_eq!(healed.get(x, y), img.get(x, y), "({x}, {y}) changed");
            }
        }
    }
}

#[test]
fn brush_is_round_on_non_square_images() {
    // 200×100: radius is a fraction of the shorter side (100) → 10 px, in
    // both axes. A marker 15 px to the right of the stroke center must stay.
    let mut img = gradient(200, 100);
    blemish(&mut img, 115.0, 50.0, 1.5, [1.0, 0.0, 0.0]);
    let healed = run(&img, json!([{ "points": [[0.5, 0.5]], "radius": 0.1 }]));
    assert_eq!(healed.get(115, 50), img.get(115, 50), "marker outside the round brush");
}

#[test]
fn empty_stroke_list_is_a_no_op() {
    let img = gradient(64, 64);
    assert_eq!(run(&img, json!([])).data, img.data);
}

#[test]
fn stroke_covering_everything_leaves_the_image_unchanged() {
    // No surrounding pixels to heal from.
    let img = gradient(16, 16);
    let out = run(&img, json!([{ "points": [[0.5, 0.5]], "radius": 1.0 }]));
    assert_eq!(out.data, img.data);
}

#[test]
fn healing_is_deterministic() {
    let mut img = gradient(180, 120);
    blemish(&mut img, 90.0, 60.0, 9.0, [0.9, 0.1, 0.1]);
    let strokes = json!([{ "points": [[0.5, 0.5], [0.52, 0.5]], "radius": 0.1 }]);
    assert_eq!(run(&img, strokes.clone()).data, run(&img, strokes).data);
}

/// Feature acceptance: preview equals export. The same recipe applied at
/// preview and 4× export resolution agrees after downscaling.
#[test]
fn preview_and_export_resolution_agree() {
    let scale = 4u32;
    let (pw, ph) = (128u32, 96u32);
    let mut small = gradient(pw, ph);
    let mut big = gradient(pw * scale, ph * scale);
    // The same blemish, in normalized coordinates, at both resolutions.
    blemish(&mut small, 0.4 * pw as f32, 0.6 * ph as f32, 3.0, [1.0, 0.0, 0.0]);
    blemish(
        &mut big,
        0.4 * (pw * scale) as f32,
        0.6 * (ph * scale) as f32,
        3.0 * scale as f32,
        [1.0, 0.0, 0.0],
    );
    let strokes = json!([{ "points": [[0.4, 0.6]], "radius": 0.06 }]);
    let a = run(&small, strokes.clone());
    let b = run(&big, strokes);
    let mut sum_sq = 0.0f64;
    for y in 0..ph {
        for x in 0..pw {
            let mut acc = [0.0f32; 3];
            for dy in 0..scale {
                for dx in 0..scale {
                    let p = b.get(x * scale + dx, y * scale + dy);
                    for c in 0..3 {
                        acc[c] += p[c];
                    }
                }
            }
            let p = a.get(x, y);
            for c in 0..3 {
                let d = ((acc[c] / (scale * scale) as f32 - p[c]).abs() * 255.0) as f64;
                sum_sq += d * d;
            }
        }
    }
    let rms = (sum_sq / (pw * ph * 3) as f64).sqrt();
    assert!(rms < 1.0, "preview vs export RMS {rms:.3}");
}

/// Deleting one stroke re-renders without the other strokes changing.
#[test]
fn removing_a_stroke_leaves_the_others_unchanged() {
    let mut img = gradient(240, 160);
    blemish(&mut img, 50.0, 40.0, 5.0, [1.0, 0.0, 0.0]);
    blemish(&mut img, 190.0, 120.0, 5.0, [0.0, 0.0, 1.0]);
    let a = json!({ "points": [[50.0 / 240.0, 40.0 / 160.0]], "radius": 0.06 });
    let b = json!({ "points": [[190.0 / 240.0, 120.0 / 160.0]], "radius": 0.06 });

    let both = run(&img, json!([a, b]));
    let only_b = run(&img, json!([b]));
    let only_a = run(&img, json!([a]));

    // B's neighbourhood is identical with or without A, and vice versa.
    assert_eq!(max_diff(&both, &only_b, 150, 80, 230, 160), 0.0);
    assert_eq!(max_diff(&both, &only_a, 0, 0, 100, 80), 0.0);
    // A's blemish is still there when only B is healed.
    assert!(max_diff(&only_b, &img, 45, 35, 55, 45) == 0.0);
}

#[test]
fn undo_all_clears_retouch() {
    let mut img = gradient(64, 64);
    blemish(&mut img, 32.0, 32.0, 3.0, [1.0, 0.0, 0.0]);
    let doc = json!({
        "spec": "fotobase.recipe", "version": 1,
        "ops": [
            { "type": "retouch", "params": { "strokes": [{ "points": [[0.5, 0.5]], "radius": 0.1 }] } },
            { "type": "undo_all" }
        ]
    });
    let (r, _) = Recipe::parse(&doc.to_string()).unwrap();
    assert_eq!(apply_recipe(&img, &r).data, img.data);
}
