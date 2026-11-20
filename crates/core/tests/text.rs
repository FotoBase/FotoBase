//! TASK-08-003: text overlay recipe op, `algo: 2` — real glyph outlines
//! with outline stroke, transparency, anchor and a font fallback
//! (docs/edit-recipe-spec-v1.md §3 `text`). The pixel-exact golden lives in
//! `crates/render/tests/recipe_golden.rs`.

use fotobase_core::apply::apply_recipe;
use fotobase_core::buffer::LinearImage;
use fotobase_core::recipe::Recipe;
use fotobase_core::text::font_substitutions;
use serde_json::{json, Value};

const BG: [f32; 3] = [0.2, 0.4, 0.6];

fn solid(w: u32, h: u32) -> LinearImage {
    LinearImage::from_fn(w, h, |_, _| BG)
}

fn recipe(ops: Value) -> Recipe {
    let doc = json!({ "spec": "fotobase.recipe", "version": 1, "ops": ops });
    Recipe::parse(&doc.to_string()).unwrap().0
}

fn text_op(extra: Value) -> Value {
    let mut params = json!({ "algo": 2, "text": "Hg", "x": 0.1, "y": 0.2, "size": 0.2 });
    for (k, v) in extra.as_object().unwrap() {
        params[k] = v.clone();
    }
    json!({ "type": "text", "params": params })
}

fn render(img: &LinearImage, ops: Value) -> LinearImage {
    apply_recipe(img, &recipe(ops))
}

/// Bounding box `(x0, y0, x1, y1)` (exclusive end) of pixels that differ
/// from `base`.
fn ink_bbox(a: &LinearImage, base: &LinearImage) -> Option<(u32, u32, u32, u32)> {
    let mut bbox: Option<(u32, u32, u32, u32)> = None;
    for y in 0..a.height {
        for x in 0..a.width {
            if a.get(x, y) != base.get(x, y) {
                bbox = Some(match bbox {
                    None => (x, y, x + 1, y + 1),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
                });
            }
        }
    }
    bbox
}

#[test]
fn same_op_renders_proportionally_at_preview_and_export_scale() {
    let ops = json!([text_op(json!({}))]);
    let (small, big) = (solid(160, 120), solid(640, 480));
    let a = ink_bbox(&render(&small, ops.clone()), &small).expect("ink at preview");
    let b = ink_bbox(&render(&big, ops), &big).expect("ink at export");
    for (s, l) in [(a.0, b.0), (a.1, b.1), (a.2, b.2), (a.3, b.3)] {
        let expected = s as i64 * 4;
        assert!(
            (l as i64 - expected).abs() <= 4,
            "bbox edge {l} at 4x should be ~{expected} (preview {s})"
        );
    }
}

#[test]
fn rendering_is_deterministic() {
    let img = solid(160, 120);
    let ops = json!([text_op(json!({ "outline": { "width": 0.01, "color": "#000000" } }))]);
    assert_eq!(render(&img, ops.clone()).data, render(&img, ops).data);
}

#[test]
fn transparency_blends_the_overlay_over_the_background() {
    let img = solid(160, 120);
    let opaque = render(&img, json!([text_op(json!({ "color": "#ffffff", "transparency": 1.0 }))]));
    let half = render(&img, json!([text_op(json!({ "color": "#ffffff", "transparency": 0.5 }))]));
    // A fully covered glyph pixel: pure white when opaque.
    let (x0, y0, x1, y1) = ink_bbox(&opaque, &img).unwrap();
    let (mut found, mut checked) = (false, false);
    for y in y0..y1 {
        for x in x0..x1 {
            if opaque.get(x, y) == [1.0, 1.0, 1.0] {
                found = true;
                let p = half.get(x, y);
                for c in 0..3 {
                    let expected = BG[c] * 0.5 + 0.5;
                    assert!((p[c] - expected).abs() < 1e-4, "half blend {p:?}");
                }
                checked = true;
            }
        }
    }
    assert!(found && checked, "opaque text must fully cover some pixel");
    // Fully transparent text changes nothing.
    let none = render(&img, json!([text_op(json!({ "transparency": 0.0 }))]));
    assert_eq!(none.data, img.data);
}

#[test]
fn outline_surrounds_the_fill_in_its_own_color() {
    let img = solid(200, 150);
    let plain = render(&img, json!([text_op(json!({ "color": "#ffffff" }))]));
    let outlined = render(
        &img,
        json!([text_op(json!({
            "color": "#ffffff",
            "outline": { "width": 0.02, "color": "#ff0000" }
        }))]),
    );
    let plain_box = ink_bbox(&plain, &img).unwrap();
    let out_box = ink_bbox(&outlined, &img).unwrap();
    assert!(out_box.0 < plain_box.0 && out_box.1 < plain_box.1, "outline grows the ink box");
    assert!(out_box.2 > plain_box.2 && out_box.3 > plain_box.3);
    // The extra pixels are the outline color, the interior stays the fill.
    let mut red_seen = false;
    for y in 0..img.height {
        for x in 0..img.width {
            let (p, q) = (plain.get(x, y), outlined.get(x, y));
            if p == [1.0, 1.0, 1.0] {
                assert_eq!(q, [1.0, 1.0, 1.0], "fill interior unchanged at ({x},{y})");
            }
            if p == BG && q == [1.0, 0.0, 0.0] {
                red_seen = true;
            }
        }
    }
    assert!(red_seen, "pure outline pixels must show the outline color");
}

#[test]
fn zero_width_outline_is_a_no_op() {
    let img = solid(160, 120);
    let plain = render(&img, json!([text_op(json!({}))]));
    let zero =
        render(&img, json!([text_op(json!({ "outline": { "width": 0.0, "color": "#ff0000" } }))]));
    assert_eq!(plain.data, zero.data);
}

#[test]
fn anchor_picks_the_point_of_the_text_block_placed_at_xy() {
    let img = solid(320, 240);
    let tl =
        ink_bbox(&render(&img, json!([text_op(json!({ "x": 0.5, "y": 0.5 }))])), &img).unwrap();
    let ctr = ink_bbox(
        &render(&img, json!([text_op(json!({ "x": 0.5, "y": 0.5, "anchor": "center" }))])),
        &img,
    )
    .unwrap();
    let br = ink_bbox(
        &render(&img, json!([text_op(json!({ "x": 0.5, "y": 0.5, "anchor": "bottom-right" }))])),
        &img,
    )
    .unwrap();
    // Top-left: ink starts at/after the anchor point.
    assert!(tl.0 >= 160 - 2 && tl.1 >= 120 - 2, "top-left ink {tl:?}");
    // Center: the point sits inside the ink box, roughly mid-way.
    assert!(ctr.0 < 160 && ctr.2 > 160 && ctr.1 < 120 && ctr.3 > 120, "center ink {ctr:?}");
    // Bottom-right: ink ends at/before the anchor point.
    assert!(br.2 <= 160 + 2 && br.3 <= 120 + 2, "bottom-right ink {br:?}");
}

#[test]
fn unknown_font_falls_back_to_the_bundled_default_and_is_recorded() {
    let img = solid(160, 120);
    let missing = render(&img, json!([text_op(json!({ "font": "No Such Font 9000" }))]));
    let sans = render(&img, json!([text_op(json!({ "font": "sans" }))]));
    assert_eq!(missing.data, sans.data, "fallback is the bundled default");

    let r = recipe(json!([
        text_op(json!({ "font": "No Such Font 9000" })),
        text_op(json!({ "font": "serif" })),
        text_op(json!({})),
    ]));
    let subs = font_substitutions(&r);
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0].op_index, 0);
    assert_eq!(subs[0].requested, "No Such Font 9000");
    assert_eq!(subs[0].used, "sans");
}

#[test]
fn bundled_families_render_differently() {
    let img = solid(200, 150);
    let sans = render(&img, json!([text_op(json!({ "font": "sans" }))]));
    let serif = render(&img, json!([text_op(json!({ "font": "serif" }))]));
    let mono = render(&img, json!([text_op(json!({ "font": "mono" }))]));
    assert_ne!(sans.data, serif.data);
    assert_ne!(sans.data, mono.data);
}

#[test]
fn re_editing_text_params_leaves_the_other_ops_untouched() {
    let img = LinearImage::from_fn(160, 120, |x, y| [x as f32 / 160.0, y as f32 / 120.0, 0.5]);
    let before = render(
        &img,
        json!([
            { "type": "sepia", "params": { "amount": 1.0 } },
            text_op(json!({ "text": "One", "x": 0.05, "y": 0.05, "size": 0.1 })),
        ]),
    );
    let after = render(
        &img,
        json!([
            { "type": "sepia", "params": { "amount": 1.0 } },
            text_op(json!({ "text": "Two", "x": 0.05, "y": 0.05, "size": 0.1, "color": "#ff0000" })),
        ]),
    );
    let sepia_only = render(&img, json!([{ "type": "sepia", "params": { "amount": 1.0 } }]));
    // Away from the text (bottom half) every render equals sepia alone.
    for y in 60..120 {
        for x in 0..160 {
            assert_eq!(before.get(x, y), sepia_only.get(x, y));
            assert_eq!(after.get(x, y), sepia_only.get(x, y));
        }
    }
    assert_ne!(before.data, after.data, "the text itself did change");
}

#[test]
fn rotation_turns_the_text_block() {
    let img = solid(240, 240);
    let flat = ink_bbox(
        &render(
            &img,
            json!([text_op(json!({ "text": "HHHH", "x": 0.5, "y": 0.5, "anchor": "center" }))]),
        ),
        &img,
    )
    .unwrap();
    let turned = ink_bbox(
        &render(
            &img,
            json!([text_op(
                json!({ "text": "HHHH", "x": 0.5, "y": 0.5, "anchor": "center", "rotation": 90 })
            )]),
        ),
        &img,
    )
    .unwrap();
    let (fw, fh) = (flat.2 - flat.0, flat.3 - flat.1);
    let (tw, th) = (turned.2 - turned.0, turned.3 - turned.1);
    assert!(fw > fh, "flat text is wider than tall");
    assert!(th > tw, "text turned 90° is taller than wide");
}

#[test]
fn newlines_start_a_new_line() {
    let img = solid(240, 240);
    let one = ink_bbox(&render(&img, json!([text_op(json!({ "text": "AB" }))])), &img).unwrap();
    let two = ink_bbox(&render(&img, json!([text_op(json!({ "text": "AB\nAB" }))])), &img).unwrap();
    assert_eq!(one.0, two.0);
    assert!(two.3 - two.1 > (one.3 - one.1) * 3 / 2, "second line adds height");
}

#[test]
fn text_without_algo_keeps_the_legacy_bitmap_look() {
    // Recipes written before algo 2 must keep rendering identically.
    let img = solid(160, 120);
    let legacy = render(
        &img,
        json!([{ "type": "text", "params": { "text": "FB", "x": 0.02, "y": 0.02, "size": 0.08, "color": "#ffffff" } }]),
    );
    let modern = render(
        &img,
        json!([{ "type": "text", "params": { "algo": 2, "text": "FB", "x": 0.02, "y": 0.02, "size": 0.08, "color": "#ffffff" } }]),
    );
    assert_ne!(legacy.data, modern.data);
    // Legacy glyphs are hard-edged: only pure white or untouched pixels.
    assert!(legacy.data.chunks(3).all(|p| p == BG || p == [1.0, 1.0, 1.0]));
}

#[test]
fn validation_rejects_bad_text_params() {
    let bad = |params: Value| {
        let doc = json!({ "spec": "fotobase.recipe", "version": 1,
            "ops": [{ "type": "text", "params": params }] });
        Recipe::parse(&doc.to_string()).is_err()
    };
    let base = json!({ "algo": 2, "text": "x" });
    let with = |k: &str, v: Value| {
        let mut p = base.clone();
        p[k] = v;
        p
    };
    assert!(!bad(base.clone()));
    assert!(bad(with("anchor", json!("middle-ish"))));
    assert!(bad(with("transparency", json!(1.5))));
    assert!(bad(with("transparency", json!(-0.1))));
    assert!(bad(with("outline", json!({ "width": 0.9, "color": "#000000" }))));
    assert!(bad(with("outline", json!({ "width": 0.01, "color": "red" }))));
    assert!(bad(with("outline", json!(3))));
    assert!(bad(with("font", json!(7))));
    assert!(bad(with("algo", json!(3))));
}

#[test]
fn documented_params_do_not_warn() {
    let doc = json!({ "spec": "fotobase.recipe", "version": 1, "ops": [text_op(json!({
        "font": "serif", "color": "#ffcc00", "rotation": 10, "anchor": "bottom",
        "outline": { "width": 0.01, "color": "#000000" }, "transparency": 0.8
    }))] });
    let (_, warnings) = Recipe::parse(&doc.to_string()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
}
