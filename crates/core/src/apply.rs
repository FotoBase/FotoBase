//! Recipe application: pure functions over a linear-light buffer
//! (docs/edit-recipe-spec-v1.md §2). One code path for preview and export —
//! the caller only chooses the resolution, the ops are identical (PLAN §3
//! rule 5). All geometry is normalized fractions of width/height, so a
//! recipe applied at any resolution yields the same image.

use serde_json::Value;

use crate::buffer::LinearImage;
use crate::recipe::{Op, Recipe};

/// Fold a recipe's ops left-to-right over a linear-light image.
pub fn apply_recipe(input: &LinearImage, recipe: &Recipe) -> LinearImage {
    let mut img = input.clone();
    for op in &recipe.ops {
        // `undo_all` clears the accumulated ops: restart from the original.
        if op.op_type == "undo_all" {
            img = input.clone();
        } else {
            img = apply_op(&img, op);
        }
    }
    img
}

fn param<'a>(op: &'a Op, key: &str) -> Option<&'a Value> {
    op.params.as_ref().and_then(|p| p.get(key))
}

fn fnum(op: &Op, key: &str, default: f64) -> f64 {
    param(op, key).and_then(Value::as_f64).unwrap_or(default)
}

fn fbool(op: &Op, key: &str, default: bool) -> bool {
    param(op, key).and_then(Value::as_bool).unwrap_or(default)
}

fn apply_op(img: &LinearImage, op: &Op) -> LinearImage {
    match op.op_type.as_str() {
        "crop" => crop(img, op),
        "straighten" => straighten(img, op),
        "auto_color" => auto_color(img),
        "auto_contrast" => auto_contrast(img),
        "fill_light" => per_pixel(img, |px| fill_light(px, fnum(op, "amount", 0.5))),
        "temperature" => per_pixel(img, |px| temperature(px, fnum(op, "kelvin_shift", 0.0))),
        "tint" => per_pixel(img, |px| tint(px, fnum(op, "green_magenta", 0.0))),
        "saturation" => per_pixel(img, |px| saturation(px, fnum(op, "amount", 0.0))),
        "brightness" => per_pixel(img, |px| brightness(px, fnum(op, "amount", 0.0))),
        "highlight_recovery" => {
            per_pixel(img, |px| highlight_recovery(px, fnum(op, "amount", 0.0)))
        }
        "shadow_recovery" => per_pixel(img, |px| shadow_recovery(px, fnum(op, "amount", 0.0))),
        "sepia" => per_pixel(img, |px| sepia(px, fnum(op, "amount", 1.0))),
        "warm_colors" => per_pixel(img, |px| temperature(px, 0.6 * fnum(op, "amount", 0.0))),
        "cool_colors" => per_pixel(img, |px| temperature(px, -0.6 * fnum(op, "amount", 0.0))),
        "red_eye" => red_eye(img, op),
        "text" => text(img, op),
        "retouch" => retouch(img, op),
        "grayscale" => {
            if fbool(op, "enabled", true) {
                per_pixel(img, grayscale)
            } else {
                img.clone()
            }
        }
        "sharpen" => sharpen(img, fnum(op, "amount", 0.5)),
        // `undo_all` is handled in `apply_recipe` (restart from the original).
        "undo_all" => img.clone(),
        // Unknown op types are skipped with a warning at parse time (spec §2).
        _ => img.clone(),
    }
}

fn per_pixel(img: &LinearImage, f: impl Fn([f32; 3]) -> [f32; 3]) -> LinearImage {
    let mut out = img.clone();
    for px in out.data.as_chunks_mut::<3>().0 {
        *px = f(*px);
    }
    out
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

fn luminance(px: [f32; 3]) -> f32 {
    0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2]
}

// ---------------------------------------------------------------------------
// Point ops
// ---------------------------------------------------------------------------

fn fill_light(px: [f32; 3], amount: f64) -> [f32; 3] {
    // Shadow lift with soft rolloff: raise dark tones proportionally to how
    // dark they are, leaving highlights untouched.
    let a = amount as f32;
    let lift = |v: f32| v + a * (1.0 - v) * (1.0 - v) * 0.5;
    [clamp01(lift(px[0])), clamp01(lift(px[1])), clamp01(lift(px[2]))]
}

fn temperature(px: [f32; 3], shift: f64) -> [f32; 3] {
    // Positive = warmer (red up, blue down); negative = cooler.
    let s = shift as f32;
    [
        clamp01(px[0] + 0.25 * s),
        clamp01(px[1] + 0.05 * s * (1.0 - s.abs())),
        clamp01(px[2] - 0.25 * s),
    ]
}

fn tint(px: [f32; 3], green_magenta: f64) -> [f32; 3] {
    let s = green_magenta as f32;
    [clamp01(px[0] + 0.15 * s), clamp01(px[1] - 0.15 * s), clamp01(px[2] + 0.15 * s)]
}

fn saturation(px: [f32; 3], amount: f64) -> [f32; 3] {
    let l = luminance(px);
    let a = amount as f32;
    let s = 1.0 + a;
    [clamp01(l + (px[0] - l) * s), clamp01(l + (px[1] - l) * s), clamp01(l + (px[2] - l) * s)]
}

fn brightness(px: [f32; 3], amount: f64) -> [f32; 3] {
    let a = amount as f32;
    let f = |v: f32| {
        if a >= 0.0 {
            // additive lift with highlight compression
            clamp01(v + a * (1.0 - v))
        } else {
            clamp01(v * (1.0 + a))
        }
    };
    [f(px[0]), f(px[1]), f(px[2])]
}

fn highlight_recovery(px: [f32; 3], amount: f64) -> [f32; 3] {
    let a = amount as f32;
    let f = |v: f32| {
        let over = (v - 0.5).max(0.0) * 2.0; // 0..1 how "highlight" it is
        clamp01(v - a * over * over * 0.35)
    };
    [f(px[0]), f(px[1]), f(px[2])]
}

fn shadow_recovery(px: [f32; 3], amount: f64) -> [f32; 3] {
    let a = amount as f32;
    let f = |v: f32| {
        let under = (0.5 - v).max(0.0) * 2.0;
        clamp01(v + a * under * under * 0.35)
    };
    [f(px[0]), f(px[1]), f(px[2])]
}

fn grayscale(px: [f32; 3]) -> [f32; 3] {
    let l = luminance(px);
    [l, l, l]
}

/// Neutral-color (white-balance) picker math (TASK-04-008, PLAN §2E Tuning
/// tab): given a linear-light sample the user says should be neutral,
/// compute the `temperature` (`kelvin_shift`) and `tint` (`green_magenta`)
/// params that make the ops neutralize it. Pure inverse of the two point
/// ops above: solving `temperature(tint(px, m), s) == gray` for `s, m`
/// gives `s = 2·(b − r)` and `m` from the r/g balance; both clamped to the
/// ops' documented [-1, 1] param range.
pub fn neutral_balance(px: [f32; 3]) -> (f64, f64) {
    let [r, g, b] = px.map(f64::from);
    let s = (2.0 * (b - r)).clamp(-1.0, 1.0);
    let m = (-((r - g) + 0.25 * s - 0.05 * s * (1.0 - s.abs())) / 0.30).clamp(-1.0, 1.0);
    (s, m)
}

/// Serializable form of [`neutral_balance`]'s result (TASK-04-008 IPC).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhiteBalance {
    pub kelvin_shift: f64,
    pub green_magenta: f64,
}

fn sepia(px: [f32; 3], amount: f64) -> [f32; 3] {
    let a = amount as f32;
    let l = luminance(px);
    // Classic sepia tone curve from grayscale luminance.
    let target = [clamp01(l * 1.07 + 0.03), clamp01(l * 0.94 + 0.02), clamp01(l * 0.72)];
    [
        clamp01(px[0] * (1.0 - a) + target[0] * a),
        clamp01(px[1] * (1.0 - a) + target[1] * a),
        clamp01(px[2] * (1.0 - a) + target[2] * a),
    ]
}

// ---------------------------------------------------------------------------
// Statistical ops (computed from the op's input at the rendered resolution —
// the one permitted resolution dependence, spec §2)
// ---------------------------------------------------------------------------

fn auto_color(img: &LinearImage) -> LinearImage {
    // Gray-world white balance: scale each channel so its mean matches the
    // overall mean, then neutralize a global color cast.
    let n = (img.width * img.height) as f32;
    let mut sum = [0.0f64; 3];
    for px in img.data.as_chunks::<3>().0 {
        for (c, v) in px.iter().enumerate() {
            sum[c] += *v as f64;
        }
    }
    let mean: [f32; 3] =
        [(sum[0] / n as f64) as f32, (sum[1] / n as f64) as f32, (sum[2] / n as f64) as f32];
    let overall = (mean[0] + mean[1] + mean[2]) / 3.0;
    let gain: [f32; 3] = [
        if mean[0] > 1e-6 { overall / mean[0] } else { 1.0 },
        if mean[1] > 1e-6 { overall / mean[1] } else { 1.0 },
        if mean[2] > 1e-6 { overall / mean[2] } else { 1.0 },
    ];
    // Blend gains toward neutral so extreme casts don't explode.
    let g = |v: f32| 1.0 + (v - 1.0).clamp(-0.6, 0.6);
    let gain = [g(gain[0]), g(gain[1]), g(gain[2])];
    per_pixel(img, |px| {
        [clamp01(px[0] * gain[0]), clamp01(px[1] * gain[1]), clamp01(px[2] * gain[2])]
    })
}

fn auto_contrast(img: &LinearImage) -> LinearImage {
    // Per-channel 1%–99% percentile stretch.
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    let mut vals: Vec<f32> = img.data.clone();
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = vals.len();
    let cutoff = (n as f32 * 0.01) as usize;
    for c in 0..3 {
        let channel: Vec<f32> = vals.iter().skip(c).step_by(3).copied().collect();
        let m = channel.len();
        lo[c] = channel[cutoff.min(m - 1)].min(0.98);
        hi[c] = channel[m - 1 - cutoff.min(m - 1)].max(0.02);
    }
    per_pixel(img, |px| {
        let f = |v: f32, c: usize| {
            if hi[c] - lo[c] < 1e-6 {
                v
            } else {
                clamp01((v - lo[c]) / (hi[c] - lo[c]))
            }
        };
        [f(px[0], 0), f(px[1], 1), f(px[2], 2)]
    })
}

// ---------------------------------------------------------------------------
// Spatial ops
// ---------------------------------------------------------------------------

fn crop(img: &LinearImage, op: &Op) -> LinearImage {
    let x0 = fnum(op, "x", 0.0);
    let y0 = fnum(op, "y", 0.0);
    let w = fnum(op, "w", 1.0);
    let h = fnum(op, "h", 1.0);
    let px0 = (x0 * img.width as f64).round() as u32;
    let py0 = (y0 * img.height as f64).round() as u32;
    let pw = ((w * img.width as f64).round() as u32).max(1);
    let ph = ((h * img.height as f64).round() as u32).max(1);
    let px0 = px0.min(img.width.saturating_sub(1));
    let py0 = py0.min(img.height.saturating_sub(1));
    let pw = pw.min(img.width - px0);
    let ph = ph.min(img.height - py0);
    let mut out = LinearImage::new(pw, ph);
    for y in 0..ph {
        for x in 0..pw {
            out.set(x, y, img.get(px0 + x, py0 + y));
        }
    }
    out
}

fn bilinear(img: &LinearImage, u: f64, v: f64) -> [f32; 3] {
    let x = (u * img.width as f64 - 0.5).clamp(0.0, img.width as f64 - 1.0);
    let y = (v * img.height as f64 - 0.5).clamp(0.0, img.height as f64 - 1.0);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(img.width - 1);
    let y1 = (y0 + 1).min(img.height - 1);
    let fx = (x - x0 as f64) as f32;
    let fy = (y - y0 as f64) as f32;
    let p00 = img.get(x0, y0);
    let p10 = img.get(x1, y0);
    let p01 = img.get(x0, y1);
    let p11 = img.get(x1, y1);
    let mut out = [0.0f32; 3];
    for c in 0..3 {
        let top = p00[c] * (1.0 - fx) + p10[c] * fx;
        let bottom = p01[c] * (1.0 - fx) + p11[c] * fx;
        out[c] = top * (1.0 - fy) + bottom * fy;
    }
    out
}

fn straighten(img: &LinearImage, op: &Op) -> LinearImage {
    let angle = fnum(op, "angle", 0.0).to_radians();
    let auto_fill = fbool(op, "auto_fill", true);
    let (w, h) = (img.width as f64, img.height as f64);
    // Rotate about the center; auto_fill scales the sampling window so the
    // rotated frame stays inside the source (no empty corners). For output
    // size (w, h) rotated by θ the required source rect is
    // w*|cos|+h*|sin| × w*|sin|+h*|cos|; scale down to fit.
    let sin = angle.sin().abs();
    let cos = angle.cos().abs();
    let need_w = w * cos + h * sin;
    let need_h = w * sin + h * cos;
    let scale = if auto_fill && (need_w > w + 1e-9 || need_h > h + 1e-9) {
        (w / need_w).min(h / need_h)
    } else {
        1.0
    };
    let mut out = LinearImage::new(img.width, img.height);
    for y in 0..img.height {
        for x in 0..img.width {
            // output pixel center in normalized units
            let dx = (x as f64 + 0.5) / w - 0.5;
            let dy = (y as f64 + 0.5) / h - 0.5;
            // inverse-rotate into source space (normalized, aspect-corrected)
            let sx0 = dx * cos + dy * sin;
            let sy0 = (-dx * sin + dy * cos) * (w / h);
            let u = sx0 / scale + 0.5;
            let v = sy0 / scale + 0.5;
            out.set(x, y, bilinear(img, u, v));
        }
    }
    out
}

/// Red-eye fix for one normalized circular region: pixels whose red channel
/// strongly dominates (the red-eye signature) are pulled down to their own
/// luminance, blended by distance from the region center so the fix fades
/// out at the rim. Pixels that don't trip the dominance test (skin, irises)
/// are left alone.
fn fix_red_region(img: &LinearImage, out: &mut LinearImage, cx: f64, cy: f64, cr: f64) {
    let rx = (cr * img.width as f64).round().max(1.0) as i64;
    let ry = (cr * img.height as f64).round().max(1.0) as i64;
    let cx_px = (cx * img.width as f64).round() as i64;
    let cy_px = (cy * img.height as f64).round() as i64;
    for dy in -ry..=ry {
        for dx in -rx..=rx {
            let x = cx_px + dx;
            let y = cy_px + dy;
            if x < 0 || y < 0 || x >= img.width as i64 || y >= img.height as i64 {
                continue;
            }
            let norm = (dx * dx) as f64 / (rx * rx) as f64 + (dy * dy) as f64 / (ry * ry) as f64;
            if norm > 1.0 {
                continue;
            }
            let px = img.get(x as u32, y as u32);
            // Red-eye signature: red strongly dominant.
            if px[0] > px[1] * 1.3 && px[0] > px[2] * 1.3 {
                let l = luminance(px);
                let fixed = [l * 0.9, l, l];
                let w = (1.0 - norm) as f32;
                let mix = |a: f32, b: f32| a * (1.0 - w) + b * w;
                out.set(
                    x as u32,
                    y as u32,
                    [mix(px[0], fixed[0]), mix(px[1], fixed[1]), mix(px[2], fixed[2])],
                );
            }
        }
    }
}

/// Auto red-eye detection heuristic (TASK-04-006): scan a coarse cell grid
/// for cells containing red-dominant pixels (`r > 1.3·g` and `r > 1.3·b`),
/// group them into 4-connected components, and keep small compact blobs —
/// pupils. Large red areas (clothing, backgrounds, saturated swatches) and
/// skin (never dominance-tripping) are not eyes. Deterministic: fixed grid,
/// fixed scan order.
fn detect_red_regions(img: &LinearImage) -> Vec<(f64, f64, f64)> {
    const CELLS: u32 = 64;
    let cw = (img.width / CELLS).max(1);
    let ch = (img.height / CELLS).max(1);
    let gw = img.width.div_ceil(cw);
    let gh = img.height.div_ceil(ch);
    let mut marks = vec![false; (gw * gh) as usize];
    for gy in 0..gh {
        for gx in 0..gw {
            let x0 = gx * cw;
            let y0 = gy * ch;
            for y in y0..(y0 + ch).min(img.height) {
                for x in x0..(x0 + cw).min(img.width) {
                    let px = img.get(x, y);
                    if px[0] > px[1] * 1.3 && px[0] > px[2] * 1.3 {
                        marks[(gy * gw + gx) as usize] = true;
                        break;
                    }
                }
                if marks[(gy * gw + gx) as usize] {
                    break;
                }
            }
        }
    }
    // 4-connected components over marked cells.
    let mut seen = vec![false; marks.len()];
    let mut out = Vec::new();
    for start in 0..marks.len() {
        if !marks[start] || seen[start] {
            continue;
        }
        let mut stack = vec![start];
        seen[start] = true;
        let mut n = 0.0f64;
        let mut cells = Vec::new();
        while let Some(idx) = stack.pop() {
            let gy = (idx as u32) / gw;
            let gx = (idx as u32) % gw;
            cells.push((gx, gy));
            n += 1.0;
            for (nx, ny) in
                [(gx.wrapping_sub(1), gy), (gx + 1, gy), (gx, gy.wrapping_sub(1)), (gx, gy + 1)]
            {
                if nx < gw && ny < gh {
                    let ni = (ny * gw + nx) as usize;
                    if marks[ni] && !seen[ni] {
                        seen[ni] = true;
                        stack.push(ni);
                    }
                }
            }
        }
        // Pupil-sized blob only: relative to the whole image, a pupil is a
        // tiny compact cluster. Cap both area and radius.
        if n / (gw * gh) as f64 > 0.005 {
            continue;
        }
        let min_gx = cells.iter().map(|c| c.0).min().unwrap();
        let max_gx = cells.iter().map(|c| c.0).max().unwrap();
        let min_gy = cells.iter().map(|c| c.1).min().unwrap();
        let max_gy = cells.iter().map(|c| c.1).max().unwrap();
        // Refine to pixel level: mean of the actual red-dominant pixels in
        // the component's bounding box gives an exact pupil center, so the
        // fix (which blends by distance) is strongest where it matters.
        let x0 = min_gx * cw;
        let y0 = min_gy * ch;
        let x1 = ((max_gx + 1) * cw).min(img.width);
        let y1 = ((max_gy + 1) * ch).min(img.height);
        let mut sum_x = 0.0f64;
        let mut sum_y = 0.0f64;
        let mut n_px = 0.0f64;
        for y in y0..y1 {
            for x in x0..x1 {
                let px = img.get(x, y);
                if px[0] > px[1] * 1.3 && px[0] > px[2] * 1.3 {
                    sum_x += x as f64;
                    sum_y += y as f64;
                    n_px += 1.0;
                }
            }
        }
        if n_px == 0.0 {
            continue;
        }
        let cx_px = sum_x / n_px;
        let cy_px = sum_y / n_px;
        let max_d = (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let px = img.get(x, y);
                px[0] > px[1] * 1.3 && px[0] > px[2] * 1.3
            })
            .map(|(x, y)| ((x as f64 - cx_px).powi(2) + (y as f64 - cy_px).powi(2)).sqrt())
            .fold(0.0f64, f64::max);
        let cx = cx_px / img.width as f64;
        let cy = cy_px / img.height as f64;
        // The region geometry is rx = r·width, ry = r·height; cover the blob
        // in both axes and cap so a stray blob can never cover a big area.
        let r = ((max_d + 1.0) / img.width as f64).clamp(0.01, 0.08);
        out.push((cx, cy, r));
    }
    out
}

fn red_eye(img: &LinearImage, op: &Op) -> LinearImage {
    let mut out = img.clone();
    if fbool(op, "auto", false) {
        for (cx, cy, cr) in detect_red_regions(img) {
            fix_red_region(img, &mut out, cx, cy, cr);
        }
        return out;
    }
    let Some(regions) = param(op, "regions").and_then(Value::as_array) else {
        return out;
    };
    for region in regions {
        let (cx, cy, cr) = (
            region["x"].as_f64().unwrap_or(0.0),
            region["y"].as_f64().unwrap_or(0.0),
            region["r"].as_f64().unwrap_or(0.0),
        );
        fix_red_region(img, &mut out, cx, cy, cr);
    }
    out
}

fn sharpen(img: &LinearImage, amount: f64) -> LinearImage {
    // Unsharp mask with a 3×3 box blur; strength scales with `amount`.
    if amount <= 0.0 {
        return img.clone();
    }
    let blurred = box_blur(img, 1);
    let a = (amount as f32).clamp(0.0, 1.0) * 1.5;
    let mut out = img.clone();
    for y in 0..img.height {
        for x in 0..img.width {
            let orig = img.get(x, y);
            let blur = blurred.get(x, y);
            let sharpened = [
                clamp01(orig[0] + (orig[0] - blur[0]) * a),
                clamp01(orig[1] + (orig[1] - blur[1]) * a),
                clamp01(orig[2] + (orig[2] - blur[2]) * a),
            ];
            out.set(x, y, sharpened);
        }
    }
    out
}

fn box_blur(img: &LinearImage, radius: u32) -> LinearImage {
    let r = radius as i64;
    let mut tmp = LinearImage::new(img.width, img.height);
    // horizontal
    for y in 0..img.height {
        for x in 0..img.width {
            let mut acc = [0.0f64; 3];
            let mut n = 0.0;
            for dx in -r..=r {
                let sx = (x as i64 + dx).clamp(0, img.width as i64 - 1) as u32;
                let p = img.get(sx, y);
                for c in 0..3 {
                    acc[c] += p[c] as f64;
                }
                n += 1.0;
            }
            tmp.set(x, y, [(acc[0] / n) as f32, (acc[1] / n) as f32, (acc[2] / n) as f32]);
        }
    }
    let mut out = LinearImage::new(img.width, img.height);
    // vertical
    for y in 0..img.height {
        for x in 0..img.width {
            let mut acc = [0.0f64; 3];
            let mut n = 0.0;
            for dy in -r..=r {
                let sy = (y as i64 + dy).clamp(0, img.height as i64 - 1) as u32;
                let p = tmp.get(x, sy);
                for c in 0..3 {
                    acc[c] += p[c] as f64;
                }
                n += 1.0;
            }
            out.set(x, y, [(acc[0] / n) as f32, (acc[1] / n) as f32, (acc[2] / n) as f32]);
        }
    }
    out
}

/// Stamp a filled disc (normalized point, pixel radii) into the stroke mask.
fn stamp_point(mask: &mut [bool], w: usize, h: usize, nx: f64, ny: f64, rx: f64, ry: f64) {
    let cx = nx * w as f64;
    let cy = ny * h as f64;
    let x0 = ((cx - rx) as i64).max(0) as usize;
    let x1 = ((cx + rx) as i64).min(w as i64 - 1) as usize;
    let y0 = ((cy - ry) as i64).max(0) as usize;
    let y1 = ((cy + ry) as i64).min(h as i64 - 1) as usize;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let nx = (x as f64 + 0.5 - cx) / rx;
            let ny = (y as f64 + 0.5 - cy) / ry;
            if nx * nx + ny * ny <= 1.0 {
                mask[y * w + x] = true;
            }
        }
    }
}

/// Classic diffusion heal: repeatedly replace pixels under the stroke with a
/// weighted blend of their neighbourhood, so texture is smoothed out while
/// the surrounding image anchors the result.
fn retouch(img: &LinearImage, op: &Op) -> LinearImage {
    let mut out = img.clone();
    let Some(strokes) = param(op, "strokes").and_then(Value::as_array) else {
        return out;
    };
    for stroke in strokes {
        let points = stroke["points"].as_array().cloned().unwrap_or_default();
        let radius = stroke["radius"].as_f64().unwrap_or(0.02);
        if points.is_empty() {
            continue;
        }
        // Mask of pixels covered by the stroke (line segments with radius).
        let w = img.width as usize;
        let h = img.height as usize;
        let mut mask = vec![false; w * h];
        let rx = (radius * img.width as f64).round().max(1.0);
        let ry = (radius * img.height as f64).round().max(1.0);
        if points.len() == 1 {
            stamp_point(
                &mut mask,
                w,
                h,
                points[0][0].as_f64().unwrap_or(0.0),
                points[0][1].as_f64().unwrap_or(0.0),
                rx,
                ry,
            );
        }
        for pair in points.windows(2) {
            let (ax, ay) = (pair[0][0].as_f64().unwrap(), pair[0][1].as_f64().unwrap());
            let (bx, by) = (pair[1][0].as_f64().unwrap(), pair[1][1].as_f64().unwrap());
            let steps =
                ((ax - bx).abs().max((ay - by).abs()) * (img.width.max(img.height) as f64) * 2.0)
                    .ceil() as usize;
            for s in 0..=steps {
                let t = if steps == 0 { 0.0 } else { s as f64 / steps as f64 };
                let cx = (ax + (bx - ax) * t) * img.width as f64;
                let cy = (ay + (by - ay) * t) * img.height as f64;
                stamp_point(&mut mask, w, h, cx / img.width as f64, cy / img.height as f64, rx, ry);
            }
        }
        // Diffuse: iteratively blend masked pixels toward the neighbourhood mean.
        for _ in 0..6 {
            let src = out.data.clone();
            for y in 0..h {
                for x in 0..w {
                    if !mask[y * w + x] {
                        continue;
                    }
                    let mut acc = [0.0f64; 3];
                    let mut weight = 0.0f64;
                    for dy in -1i64..=1 {
                        for dx in -1i64..=1 {
                            let sx = (x as i64 + dx).clamp(0, w as i64 - 1) as usize;
                            let sy = (y as i64 + dy).clamp(0, h as i64 - 1) as usize;
                            let wgt = if mask[sy * w + sx] { 0.25 } else { 1.0 };
                            let i = (sy * w + sx) * 3;
                            for (c, v) in src[i..i + 3].iter().enumerate() {
                                acc[c] += *v as f64 * wgt;
                            }
                            weight += wgt;
                        }
                    }
                    let i = (y * w + x) * 3;
                    for (c, v) in out.data[i..i + 3].iter_mut().enumerate() {
                        *v = (acc[c] / weight) as f32;
                    }
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Text rasterization (deterministic embedded 5×7 bitmap font, spec §3:
// "v1: rasterized at export; params normalized")
// ---------------------------------------------------------------------------

/// 5×7 column-packed glyph bitmaps for A–Z, 0–9, space and a few marks.
const FONT: &[(char, [u8; 5])] = &[
    ('A', [0x7E, 0x11, 0x11, 0x11, 0x7E]),
    ('B', [0x7F, 0x49, 0x49, 0x49, 0x36]),
    ('C', [0x3E, 0x41, 0x41, 0x41, 0x22]),
    ('D', [0x7F, 0x41, 0x41, 0x22, 0x1C]),
    ('E', [0x7F, 0x49, 0x49, 0x49, 0x41]),
    ('F', [0x7F, 0x09, 0x09, 0x09, 0x01]),
    ('G', [0x3E, 0x41, 0x49, 0x49, 0x3A]),
    ('H', [0x7F, 0x08, 0x08, 0x08, 0x7F]),
    ('I', [0x00, 0x41, 0x7F, 0x41, 0x00]),
    ('J', [0x20, 0x40, 0x41, 0x3F, 0x01]),
    ('K', [0x7F, 0x08, 0x14, 0x22, 0x41]),
    ('L', [0x7F, 0x40, 0x40, 0x40, 0x40]),
    ('M', [0x7F, 0x02, 0x04, 0x02, 0x7F]),
    ('N', [0x7F, 0x04, 0x08, 0x10, 0x7F]),
    ('O', [0x3E, 0x41, 0x41, 0x41, 0x3E]),
    ('P', [0x7F, 0x09, 0x09, 0x09, 0x06]),
    ('Q', [0x3E, 0x41, 0x51, 0x21, 0x5E]),
    ('R', [0x7F, 0x09, 0x19, 0x29, 0x46]),
    ('S', [0x46, 0x49, 0x49, 0x49, 0x31]),
    ('T', [0x01, 0x01, 0x7F, 0x01, 0x01]),
    ('U', [0x3F, 0x40, 0x40, 0x40, 0x3F]),
    ('V', [0x1F, 0x20, 0x40, 0x20, 0x1F]),
    ('W', [0x7F, 0x20, 0x18, 0x20, 0x7F]),
    ('X', [0x63, 0x14, 0x08, 0x14, 0x63]),
    ('Y', [0x03, 0x04, 0x78, 0x04, 0x03]),
    ('Z', [0x61, 0x51, 0x49, 0x45, 0x43]),
    ('0', [0x3E, 0x51, 0x49, 0x45, 0x3E]),
    ('1', [0x00, 0x42, 0x7F, 0x40, 0x00]),
    ('2', [0x42, 0x61, 0x51, 0x49, 0x46]),
    ('3', [0x21, 0x41, 0x45, 0x4B, 0x31]),
    ('4', [0x18, 0x14, 0x12, 0x7F, 0x10]),
    ('5', [0x27, 0x45, 0x45, 0x45, 0x39]),
    ('6', [0x3C, 0x4A, 0x49, 0x49, 0x30]),
    ('7', [0x01, 0x71, 0x09, 0x05, 0x03]),
    ('8', [0x36, 0x49, 0x49, 0x49, 0x36]),
    ('9', [0x06, 0x49, 0x49, 0x29, 0x1E]),
    (' ', [0x00, 0x00, 0x00, 0x00, 0x00]),
    ('.', [0x00, 0x60, 0x60, 0x00, 0x00]),
    (',', [0x00, 0x50, 0x30, 0x00, 0x00]),
    ('-', [0x08, 0x08, 0x08, 0x08, 0x08]),
    ('!', [0x00, 0x00, 0x5F, 0x00, 0x00]),
    ('?', [0x02, 0x01, 0x51, 0x09, 0x06]),
    (':', [0x00, 0x36, 0x36, 0x00, 0x00]),
    ('/', [0x20, 0x10, 0x08, 0x04, 0x02]),
    ('(', [0x00, 0x1C, 0x22, 0x41, 0x00]),
    (')', [0x00, 0x41, 0x22, 0x1C, 0x00]),
];

/// Parse `#rrggbb` (or `#rgb`) into linear-light-ish [0, 1] components.
pub fn parse_hex_color(hex: &str) -> Option<[f32; 3]> {
    let h = hex.strip_prefix('#')?;
    let (r, g, b) = match h.len() {
        6 => (
            u8::from_str_radix(&h[0..2], 16).ok()?,
            u8::from_str_radix(&h[2..4], 16).ok()?,
            u8::from_str_radix(&h[4..6], 16).ok()?,
        ),
        3 => (
            u8::from_str_radix(&h[0..1], 16).ok()? * 17,
            u8::from_str_radix(&h[1..2], 16).ok()? * 17,
            u8::from_str_radix(&h[2..3], 16).ok()? * 17,
        ),
        _ => return None,
    };
    Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0])
}

fn glyph(ch: char) -> Option<&'static [u8; 5]> {
    let upper = ch.to_ascii_uppercase();
    FONT.iter().find(|(c, _)| *c == upper).map(|(_, bits)| bits)
}

fn text(img: &LinearImage, op: &Op) -> LinearImage {
    let mut out = img.clone();
    let Some(s) = param(op, "text").and_then(Value::as_str) else {
        return out;
    };
    let x = fnum(op, "x", 0.0);
    let y = fnum(op, "y", 0.0);
    let size = fnum(op, "size", 0.05);
    let rotation = fnum(op, "rotation", 0.0).to_radians();
    let color: [f32; 3] = match param(op, "color") {
        Some(Value::String(hex)) => parse_hex_color(hex).unwrap_or([1.0, 1.0, 1.0]),
        Some(Value::Array(c)) if c.len() == 3 => [
            c[0].as_f64().unwrap_or(1.0) as f32,
            c[1].as_f64().unwrap_or(1.0) as f32,
            c[2].as_f64().unwrap_or(1.0) as f32,
        ],
        _ => [1.0, 1.0, 1.0],
    };
    if s.is_empty() || size <= 0.0 {
        return out;
    }
    // Glyph cell in pixels: 7 rows tall = `size * height`, 6 px advance.
    let cell = (size * img.height as f64).max(3.0);
    let advance = cell * 6.0 / 7.0;
    let width_px = advance * s.chars().count() as f64;
    let height_px = cell;
    // Anchor: (x, y) is the top-left of the text block (normalized).
    let ox = x * img.width as f64;
    let oy = y * img.height as f64;
    for py in 0..img.height {
        for px in 0..img.width {
            // position relative to anchor, un-rotated
            let fx = px as f64 + 0.5 - ox;
            let fy = py as f64 + 0.5 - oy;
            let lx = fx * rotation.cos() + fy * rotation.sin();
            let ly = -fx * rotation.sin() + fy * rotation.cos();
            if lx < 0.0 || ly < 0.0 || lx >= width_px || ly >= height_px {
                continue;
            }
            let char_idx = (lx / advance) as usize;
            let ch = s.chars().nth(char_idx.min(s.chars().count() - 1));
            let Some(bits) = ch.and_then(glyph) else { continue };
            let col = ((lx - char_idx as f64 * advance) / (cell / 7.0)) as usize;
            let row = (ly / (cell / 7.0)) as usize;
            if col >= 5 || row >= 7 {
                continue;
            }
            if bits[col] & (1 << row) == 0 {
                continue;
            }
            let mut p = out.get(px, py);
            for c in 0..3 {
                p[c] = clamp01(p[c] * (1.0 - color[c]) + color[c]);
            }
            out.set(px, py, p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn img() -> LinearImage {
        LinearImage::from_fn(4, 4, |x, y| [(x + y) as f32 / 6.0, 0.5, 1.0 - (x + y) as f32 / 6.0])
    }

    fn op(t: &str, params: serde_json::Value) -> Op {
        let v = json!({ "type": t, "params": params });
        let o: Op = serde_json::from_value(v).unwrap();
        o
    }

    // TASK-04-008: neutral-color picker math. `neutral_balance` must return
    // identity params for an already-neutral pixel and params that make the
    // temperature/tint ops neutralize a cast sample.
    #[test]
    fn neutral_picker_on_gray_pixel_yields_identity() {
        for v in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let (kelvin_shift, green_magenta) = neutral_balance([v, v, v]);
            assert_eq!(kelvin_shift, 0.0);
            assert_eq!(green_magenta, 0.0);
        }
    }

    #[test]
    fn neutral_picker_params_neutralize_a_warm_cast() {
        let sample = [0.6f32, 0.45, 0.3];
        let (s, m) = neutral_balance(sample);
        let out = temperature(tint(sample, m), s);
        assert!((out[0] - out[1]).abs() < 1e-3, "r≈g after balance: {out:?}");
        assert!((out[1] - out[2]).abs() < 1e-3, "g≈b after balance: {out:?}");
    }

    #[test]
    fn neutral_picker_params_neutralize_a_green_cast() {
        let sample = [0.4f32, 0.55, 0.42];
        let (s, m) = neutral_balance(sample);
        let out = temperature(tint(sample, m), s);
        assert!((out[0] - out[1]).abs() < 1e-3, "r≈g after balance: {out:?}");
        assert!((out[1] - out[2]).abs() < 1e-3, "g≈b after balance: {out:?}");
    }

    #[test]
    fn neutral_picker_params_stay_in_range() {
        for sample in [[0.9f32, 0.1, 0.1], [0.05, 0.9, 0.05], [0.1, 0.1, 0.9]] {
            let (s, m) = neutral_balance(sample);
            assert!((-1.0..=1.0).contains(&s));
            assert!((-1.0..=1.0).contains(&m));
        }
    }

    #[test]
    fn sepia_full_amount_matches_target_curve() {
        let out = per_pixel(&img(), |px| sepia(px, 1.0));
        for px in out.data.as_chunks::<3>().0 {
            assert!(px[0] >= px[1] && px[1] >= px[2], "sepia ordering");
        }
    }

    #[test]
    fn crop_bounds_are_clamped_safely() {
        let i = img();
        let out = crop(&i, &op("crop", json!({ "x": 0.5, "y": 0.5, "w": 1.0, "h": 1.0 })));
        assert_eq!((out.width, out.height), (2, 2));
    }

    #[test]
    fn straighten_auto_fill_has_no_empty_corners() {
        let i = img();
        let out = straighten(&i, &op("straighten", json!({ "angle": 10.0, "auto_fill": true })));
        // corners must come from real pixels (gradient values), not black
        for (x, y) in [(0, 0), (3, 0), (0, 3), (3, 3)] {
            let p = out.get(x, y);
            assert!(p[0] > 0.0 || p[2] > 0.0, "corner ({x},{y}) not empty");
        }
    }

    #[test]
    fn text_is_rasterized_deterministically() {
        let i = img();
        let o = op("text", json!({ "text": "AB", "x": 0.1, "y": 0.1, "size": 0.3 }));
        let a = text(&i, &o);
        let b = text(&i, &o);
        assert_eq!(a.data, b.data);
        assert!(a.data.iter().any(|&v| v != 0.0) && a.data != i.data);
    }

    #[test]
    fn retouch_smooths_the_stroked_area() {
        // high-contrast checker
        let i = LinearImage::from_fn(8, 8, |x, y| {
            let v = if (x + y) % 2 == 0 { 1.0 } else { 0.0 };
            [v, v, v]
        });
        let o = op("retouch", json!({ "strokes": [{ "points": [[0.5, 0.5]], "radius": 0.1 }] }));
        let out = retouch(&i, &o);
        let center = out.get(4, 4);
        assert!(center[0] > 0.2 && center[0] < 0.8, "healed pixel is a blend: {center:?}");
    }

    #[test]
    fn sharpen_increases_local_contrast() {
        let i = LinearImage::from_fn(8, 8, |x, _| {
            let v = if x < 4 { 0.2 } else { 0.8 };
            [v, v, v]
        });
        let out = sharpen(&i, 1.0);
        assert!(out.get(3, 0)[0] < 0.2, "dark side gets darker at the edge");
        assert!(out.get(4, 0)[0] > 0.8, "bright side gets brighter at the edge");
    }
}
