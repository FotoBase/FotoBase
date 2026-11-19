//! TASK-08-001: remaining effects (Warmify, Film Grain, Tint, Soft Focus,
//! Vignette) and frames (Border, Drop Shadow) as recipe ops (PLAN §2E
//! Effects, §6 P4; docs/edit-recipe-spec-v1.md §3). Engine-level behavior;
//! pixel-exact goldens live in `crates/render/tests/recipe_golden.rs`.

use fotobase_core::apply::apply_recipe;
use fotobase_core::buffer::LinearImage;
use fotobase_core::history::EditHistory;
use fotobase_core::recipe::Recipe;
use serde_json::{json, Value};

fn recipe(ops: Value) -> Recipe {
    let doc = json!({ "spec": "fotobase.recipe", "version": 1, "ops": ops });
    Recipe::parse(&doc.to_string()).unwrap().0
}

fn parse_err(op: Value) -> String {
    let doc = json!({ "spec": "fotobase.recipe", "version": 1, "ops": [op] });
    Recipe::parse(&doc.to_string()).unwrap_err().to_string()
}

fn flat(w: u32, h: u32, px: [f32; 3]) -> LinearImage {
    LinearImage::from_fn(w, h, |_, _| px)
}

fn gradient(w: u32, h: u32) -> LinearImage {
    LinearImage::from_fn(w, h, |x, y| {
        [
            0.2 + 0.6 * x as f32 / w as f32,
            0.2 + 0.6 * y as f32 / h as f32,
            0.4 + 0.2 * (x as f32 / w as f32) * (y as f32 / h as f32),
        ]
    })
}

fn checker(w: u32, h: u32) -> LinearImage {
    LinearImage::from_fn(w, h, |x, y| if (x + y) % 2 == 0 { [0.9; 3] } else { [0.1; 3] })
}

fn run(img: &LinearImage, ops: Value) -> LinearImage {
    apply_recipe(img, &recipe(ops))
}

fn lum(px: [f32; 3]) -> f32 {
    0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2]
}

/// Mean absolute difference between horizontally adjacent pixels.
fn roughness(img: &LinearImage) -> f32 {
    let mut sum = 0.0;
    let mut n = 0.0;
    for y in 0..img.height {
        for x in 1..img.width {
            sum += (lum(img.get(x, y)) - lum(img.get(x - 1, y))).abs();
            n += 1.0;
        }
    }
    sum / n
}

const NEW_OPS: &[&str] = &["warmify", "soft_focus", "vignette", "border", "drop_shadow"];

#[test]
fn new_op_types_parse_without_warnings_and_round_trip() {
    for op in NEW_OPS {
        let doc = json!({ "spec": "fotobase.recipe", "version": 1,
                          "ops": [{ "type": op, "params": { "amount": 0.4 } }] });
        let (r, warnings) = Recipe::parse(&doc.to_string()).unwrap();
        assert!(warnings.is_empty(), "{op}: {warnings:?}");
        assert_eq!(r.ops.len(), 1, "{op} must not be skipped as unknown");
        assert_eq!(Recipe::parse(&r.to_json()).unwrap().0, r);
    }
}

#[test]
fn out_of_range_params_are_rejected() {
    for op in NEW_OPS {
        let msg = parse_err(json!({ "type": op, "params": { "amount": 1.5 } }));
        assert!(msg.contains(op) && msg.contains("amount"), "{msg}");
        parse_err(json!({ "type": op, "params": { "amount": -0.1 } }));
    }
    parse_err(json!({ "type": "tint", "params": { "color": "red", "amount": 0.5 } }));
    parse_err(json!({ "type": "tint", "params": { "color": "#ff0000", "amount": 2.0 } }));
    parse_err(json!({ "type": "border", "params": { "color": 5 } }));
    parse_err(json!({ "type": "drop_shadow", "params": { "color": "#12" } }));
    parse_err(json!({ "type": "film_grain", "params": { "amount": 2.0 } }));
}

#[test]
fn colorless_tint_keeps_its_white_balance_meaning() {
    let img = gradient(8, 8);
    let wb = run(&img, json!([{ "type": "tint", "params": { "green_magenta": 0.5 } }]));
    assert_ne!(wb, img);
    // `color` never leaks into a recipe that only carries green_magenta.
    let (r, warnings) = Recipe::parse(
        &json!({ "spec": "fotobase.recipe", "version": 1,
                 "ops": [{ "type": "tint", "params": { "green_magenta": 0.1 } }] })
        .to_string(),
    )
    .unwrap();
    assert!(warnings.is_empty());
    assert!(r.ops[0].params.as_ref().unwrap().get("color").is_none());
}

#[test]
fn zero_strength_is_bit_identical_for_every_effect() {
    let img = gradient(16, 12);
    for op in [
        json!({ "type": "warmify", "params": { "amount": 0.0 } }),
        json!({ "type": "tint", "params": { "color": "#3a7bd5", "amount": 0.0 } }),
        json!({ "type": "soft_focus", "params": { "amount": 0.0 } }),
        json!({ "type": "vignette", "params": { "amount": 0.0 } }),
        json!({ "type": "border", "params": { "amount": 0.0 } }),
        json!({ "type": "drop_shadow", "params": { "amount": 0.0 } }),
    ] {
        assert_eq!(run(&img, json!([op.clone()])), img, "{op}");
    }
}

#[test]
fn warmify_shifts_toward_red_and_away_from_blue() {
    let img = flat(4, 4, [0.4, 0.4, 0.4]);
    let out = run(&img, json!([{ "type": "warmify", "params": { "amount": 0.6 } }]));
    let px = out.get(1, 1);
    assert!(px[0] > 0.4 && px[2] < 0.4, "{px:?}");
    let stronger = run(&img, json!([{ "type": "warmify", "params": { "amount": 1.0 } }])).get(1, 1);
    assert!(stronger[0] > px[0] && stronger[2] < px[2]);
    assert!(out.data.iter().all(|v| (0.0..=1.0).contains(v)));
}

#[test]
fn tint_with_color_washes_toward_it_and_keeps_brightness() {
    let img = flat(4, 4, [0.5, 0.5, 0.5]);
    let blue = json!([{ "type": "tint", "params": { "color": "#0000ff", "amount": 1.0 } }]);
    let px = run(&img, blue).get(0, 0);
    assert!(px[2] > px[0] && px[2] > px[1], "blue wash: {px:?}");
    let half =
        run(&img, json!([{ "type": "tint", "params": { "color": "#ff8000", "amount": 0.5 } }]))
            .get(0, 0);
    let full =
        run(&img, json!([{ "type": "tint", "params": { "color": "#ff8000", "amount": 1.0 } }]))
            .get(0, 0);
    assert!(full[0] - full[2] > half[0] - half[2], "strength scales the wash");
}

#[test]
fn vignette_darkens_corners_and_leaves_the_center() {
    let img = flat(40, 30, [0.6, 0.6, 0.6]);
    let out = run(&img, json!([{ "type": "vignette", "params": { "amount": 0.8 } }]));
    assert_eq!(out.get(20, 15), [0.6; 3], "center untouched");
    let corner = out.get(0, 0);
    assert!(corner[0] < 0.5, "corner darkened: {corner:?}");
    let edge = out.get(20, 0);
    assert!(edge[0] < 0.6 && edge[0] > corner[0], "edges fall between center and corner");
    // radial symmetry
    assert_eq!(out.get(0, 0), out.get(39, 29));
    let weaker = run(&img, json!([{ "type": "vignette", "params": { "amount": 0.3 } }]));
    assert!(weaker.get(0, 0)[0] > corner[0]);
}

#[test]
fn soft_focus_blurs_detail_but_keeps_flat_areas() {
    let flat_img = flat(32, 32, [0.5; 3]);
    let out = run(&flat_img, json!([{ "type": "soft_focus", "params": { "amount": 1.0 } }]));
    for (a, b) in out.data.iter().zip(flat_img.data.iter()) {
        assert!((a - b).abs() < 0.2, "flat region stays near its value");
    }
    let img = checker(32, 32);
    let soft = run(&img, json!([{ "type": "soft_focus", "params": { "amount": 1.0 } }]));
    assert!(roughness(&soft) < roughness(&img) * 0.6, "detail is softened");
    let mild = run(&img, json!([{ "type": "soft_focus", "params": { "amount": 0.3 } }]));
    assert!(roughness(&mild) > roughness(&soft), "strength scales the softening");
}

#[test]
fn film_grain_is_reproducible_and_seed_dependent() {
    let img = flat(24, 24, [0.5; 3]);
    let op =
        |seed: u64| json!([{ "type": "film_grain", "params": { "amount": 0.6, "seed": seed } }]);
    let a = run(&img, op(7));
    assert_eq!(a, run(&img, op(7)), "same seed → same pixels");
    assert_ne!(a, run(&img, op(8)), "different seed → different pixels");
    assert_ne!(a, img);
}

#[test]
fn border_grows_the_canvas_and_paints_the_frame() {
    let img = flat(100, 60, [0.3, 0.3, 0.3]);
    // amount 0.5 → 5% of the shorter side (60) = 3 px.
    let out =
        run(&img, json!([{ "type": "border", "params": { "amount": 0.5, "color": "#ff0000" } }]));
    assert_eq!((out.width, out.height), (106, 66));
    assert_eq!(out.get(0, 0), [1.0, 0.0, 0.0]);
    assert_eq!(out.get(105, 65), [1.0, 0.0, 0.0]);
    assert_eq!(out.get(3, 3), [0.3, 0.3, 0.3], "photo sits inside the frame");
    assert_eq!(out.get(102, 62), [0.3, 0.3, 0.3]);
    assert_eq!(out.get(2, 30), [1.0, 0.0, 0.0]);
    // default color is white
    let white = run(&img, json!([{ "type": "border", "params": { "amount": 0.5 } }]));
    assert_eq!(white.get(0, 0), [1.0; 3]);
}

#[test]
fn drop_shadow_adds_a_margin_with_a_shadow_behind_the_photo() {
    let img = flat(80, 60, [0.9; 3]);
    // amount 1.0 → margin = 10% of 60 = 6 px.
    let out = run(&img, json!([{ "type": "drop_shadow", "params": { "amount": 1.0 } }]));
    assert_eq!((out.width, out.height), (92, 72));
    assert_eq!(out.get(0, 0), [1.0; 3], "far corner keeps the background");
    assert_eq!(out.get(6, 6), [0.9; 3], "photo untouched");
    assert_eq!(out.get(40, 30), [0.9; 3]);
    // Shadow is offset toward the bottom-right: the band below the photo is
    // darker than the band above it.
    let below = lum(out.get(46, 6 + 60 + 1));
    let above = lum(out.get(46, 3));
    assert!(below < above - 0.05, "shadow below {below} vs above {above}");
    let right = lum(out.get(6 + 80 + 1, 36));
    let left = lum(out.get(3, 36));
    assert!(right < left - 0.05, "shadow right {right} vs left {left}");
}

#[test]
fn frames_compose_with_effects_in_one_recipe() {
    let img = gradient(64, 48);
    let ops = json!([
        { "type": "sepia", "params": { "amount": 0.4 } },
        { "type": "warmify", "params": { "amount": 0.5 } },
        { "type": "tint", "params": { "color": "#3a7bd5", "amount": 0.2 } },
        { "type": "film_grain", "params": { "amount": 0.3, "seed": 1 } },
        { "type": "soft_focus", "params": { "amount": 0.5 } },
        { "type": "vignette", "params": { "amount": 0.5 } },
        { "type": "border", "params": { "amount": 0.4 } },
        { "type": "drop_shadow", "params": { "amount": 0.5 } },
    ]);
    let out = run(&img, ops.clone());
    // border: round(0.4*0.1*48)=2 → 68x52; drop_shadow: round(0.5*0.1*52)=3 → 74x58.
    assert_eq!((out.width, out.height), (74, 58));
    assert!(out.data.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
    assert_eq!(out, run(&img, ops), "ordered fold is deterministic");
    // Order matters: the vignette must act on the photo, not the frame.
    let swapped = run(
        &img,
        json!([
            { "type": "border", "params": { "amount": 0.4 } },
            { "type": "vignette", "params": { "amount": 0.5 } },
        ]),
    );
    let framed_first = run(
        &img,
        json!([
            { "type": "vignette", "params": { "amount": 0.5 } },
            { "type": "border", "params": { "amount": 0.4 } },
        ]),
    );
    assert_ne!(swapped, framed_first);
}

#[test]
fn undo_redo_and_revert_work_with_the_new_ops() {
    let saved = recipe(json!([{ "type": "vignette", "params": { "amount": 0.4 } }]));
    let with_warm = recipe(json!([
        { "type": "vignette", "params": { "amount": 0.4 } },
        { "type": "warmify", "params": { "amount": 0.5 } },
    ]));
    let with_frame = recipe(json!([
        { "type": "vignette", "params": { "amount": 0.4 } },
        { "type": "warmify", "params": { "amount": 0.5 } },
        { "type": "border", "params": { "amount": 0.5 } },
    ]));
    let mut h = EditHistory::new(Some(saved.clone()));
    h.apply(&with_warm);
    h.apply(&with_frame);
    h.undo();
    assert_eq!(h.current(), Some(&with_warm));
    h.redo();
    assert_eq!(h.current(), Some(&with_frame));
    h.revert();
    assert_eq!(h.current(), Some(&saved));

    // `undo_all` mid-recipe drops everything before it, frames included.
    let img = gradient(32, 24);
    let cleared = run(
        &img,
        json!([
            { "type": "border", "params": { "amount": 0.5 } },
            { "type": "undo_all" },
        ]),
    );
    assert_eq!(cleared, img);
}
