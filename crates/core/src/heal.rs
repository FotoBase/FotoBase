//! Heal brush (TASK-08-002, PLAN §2E Retouch): classic diffusion heal.
//!
//! Pixels under a stroke are replaced by the solution of Laplace's equation
//! with the surrounding pixels as boundary values (harmonic inpainting), so
//! a blemish dissolves into its neighbourhood and any linear gradient is
//! reproduced exactly. The solve is a nested iteration over a 2× pyramid —
//! coarse solution → bilinear prolongation → a few Gauss–Seidel sweeps — so
//! the cost and the result do not depend on how large the stroke is in
//! pixels (preview and export converge to the same image).
//!
//! Pure and deterministic: no randomness, fixed sweep counts, fixed
//! traversal order. Strokes are healed one after the other, each from the
//! pixels of its own bounding box plus a one-pixel ring, so strokes that do
//! not touch each other are independent of one another.

use serde_json::Value;

use crate::buffer::LinearImage;

/// Below this size the grid is solved directly.
const COARSEST: usize = 32;
/// Over-relaxation for the coarsest grid (optimal for ~32×32).
const COARSE_OMEGA: f32 = 1.8;
const COARSE_SWEEPS: usize = 200;
/// Plain Gauss–Seidel sweeps after each prolongation.
const FINE_SWEEPS: usize = 30;

/// Heal every stroke of a `retouch` op in order. Stroke geometry is
/// normalized: points are fractions of the width/height, `radius` is a
/// fraction of the shorter side (a round brush at any aspect ratio).
pub fn heal_strokes(img: &LinearImage, strokes: &[Value]) -> LinearImage {
    let mut out = img.clone();
    for stroke in strokes {
        let points: Vec<(f64, f64)> = stroke["points"]
            .as_array()
            .map(|pts| {
                pts.iter()
                    .filter_map(|p| Some((p.get(0)?.as_f64()?, p.get(1)?.as_f64()?)))
                    .collect()
            })
            .unwrap_or_default();
        let radius = stroke["radius"].as_f64().unwrap_or(0.02);
        heal_stroke(&mut out, &points, radius);
    }
    out
}

fn heal_stroke(img: &mut LinearImage, points: &[(f64, f64)], radius: f64) {
    if points.is_empty() || img.width == 0 || img.height == 0 {
        return;
    }
    let (w, h) = (img.width as usize, img.height as usize);
    let (wf, hf) = (w as f64, h as f64);
    // A radius below a pixel still heals the pixel under the point.
    let r = (radius * wf.min(hf)).max(1.0);
    let pts: Vec<(f64, f64)> = points.iter().map(|&(x, y)| (x * wf, y * hf)).collect();

    // Region of interest: stroke bounding box, plus a one-pixel ring of
    // known pixels to heal from.
    let (mut min_x, mut min_y) = (f64::MAX, f64::MAX);
    let (mut max_x, mut max_y) = (f64::MIN, f64::MIN);
    for &(x, y) in &pts {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    let x0 = ((min_x - r).floor() as i64 - 1).clamp(0, w as i64 - 1) as usize;
    let y0 = ((min_y - r).floor() as i64 - 1).clamp(0, h as i64 - 1) as usize;
    let x1 = ((max_x + r).ceil() as i64 + 1).clamp(1, w as i64) as usize;
    let y1 = ((max_y + r).ceil() as i64 + 1).clamp(1, h as i64) as usize;
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let (rw, rh) = (x1 - x0, y1 - y0);

    let mut mask = vec![false; rw * rh];
    let mut covered = 0usize;
    let mut stamp = |a: (f64, f64), b: (f64, f64)| {
        let sx0 = ((a.0.min(b.0) - r).floor() as i64).max(x0 as i64) as usize;
        let sx1 = ((a.0.max(b.0) + r).ceil() as i64).min(x1 as i64) as usize;
        let sy0 = ((a.1.min(b.1) - r).floor() as i64).max(y0 as i64) as usize;
        let sy1 = ((a.1.max(b.1) + r).ceil() as i64).min(y1 as i64) as usize;
        for y in sy0..sy1 {
            for x in sx0..sx1 {
                let m = &mut mask[(y - y0) * rw + (x - x0)];
                if !*m && dist_sq_to_segment(x as f64 + 0.5, y as f64 + 0.5, a, b) <= r * r {
                    *m = true;
                    covered += 1;
                }
            }
        }
    };
    if pts.len() == 1 {
        stamp(pts[0], pts[0]);
    }
    for pair in pts.windows(2) {
        stamp(pair[0], pair[1]);
    }
    // Nothing to heal, or nothing to heal from.
    if covered == 0 || covered == rw * rh {
        return;
    }

    let mut region: Vec<[f32; 3]> = Vec::with_capacity(rw * rh);
    for y in y0..y1 {
        for x in x0..x1 {
            region.push(img.get(x as u32, y as u32));
        }
    }
    solve(&mut region, &mask, rw, rh);
    for y in 0..rh {
        for x in 0..rw {
            if mask[y * rw + x] {
                img.set((x0 + x) as u32, (y0 + y) as u32, region[y * rw + x]);
            }
        }
    }
}

fn dist_sq_to_segment(px: f64, py: f64, a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq == 0.0 {
        0.0
    } else {
        (((px - a.0) * dx + (py - a.1) * dy) / len_sq).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a.0 + t * dx, a.1 + t * dy);
    (px - cx).powi(2) + (py - cy).powi(2)
}

/// Fill the masked pixels of `px` (w×h) with the harmonic interpolation of
/// the unmasked ones. At least one pixel must be unmasked.
fn solve(px: &mut [[f32; 3]], mask: &[bool], w: usize, h: usize) {
    if w.max(h) <= COARSEST {
        let (mut sum, mut n) = ([0.0f64; 3], 0usize);
        for (p, _) in px.iter().zip(mask).filter(|(_, m)| !**m) {
            for c in 0..3 {
                sum[c] += p[c] as f64;
            }
            n += 1;
        }
        let mean = sum.map(|s| (s / n as f64) as f32);
        for (p, _) in px.iter_mut().zip(mask).filter(|(_, m)| **m) {
            *p = mean;
        }
        relax(px, mask, w, h, COARSE_SWEEPS, COARSE_OMEGA);
        return;
    }

    // Coarse grid: a cell is known when any of its pixels is (average of
    // the known ones), unknown only when every pixel under it is masked.
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let mut cpx = vec![[0.0f32; 3]; cw * ch];
    let mut cmask = vec![true; cw * ch];
    for cy in 0..ch {
        for cx in 0..cw {
            let (mut sum, mut n) = ([0.0f32; 3], 0u32);
            for y in (2 * cy)..(2 * cy + 2).min(h) {
                for x in (2 * cx)..(2 * cx + 2).min(w) {
                    if !mask[y * w + x] {
                        for c in 0..3 {
                            sum[c] += px[y * w + x][c];
                        }
                        n += 1;
                    }
                }
            }
            if n > 0 {
                cpx[cy * cw + cx] = sum.map(|s| s / n as f32);
                cmask[cy * cw + cx] = false;
            }
        }
    }
    solve(&mut cpx, &cmask, cw, ch);

    // Prolongation: bilinear sample of the coarse solution.
    for y in 0..h {
        for x in 0..w {
            if !mask[y * w + x] {
                continue;
            }
            let fx = ((x as f32 + 0.5) / 2.0 - 0.5).clamp(0.0, (cw - 1) as f32);
            let fy = ((y as f32 + 0.5) / 2.0 - 0.5).clamp(0.0, (ch - 1) as f32);
            let (ix, iy) = (fx.floor() as usize, fy.floor() as usize);
            let (jx, jy) = ((ix + 1).min(cw - 1), (iy + 1).min(ch - 1));
            let (tx, ty) = (fx - ix as f32, fy - iy as f32);
            let mut v = [0.0f32; 3];
            for c in 0..3 {
                let top = cpx[iy * cw + ix][c] * (1.0 - tx) + cpx[iy * cw + jx][c] * tx;
                let bottom = cpx[jy * cw + ix][c] * (1.0 - tx) + cpx[jy * cw + jx][c] * tx;
                v[c] = top * (1.0 - ty) + bottom * ty;
            }
            px[y * w + x] = v;
        }
    }
    relax(px, mask, w, h, FINE_SWEEPS, 1.0);
}

/// Gauss–Seidel / SOR sweeps over the masked pixels: each becomes the mean
/// of its existing 4-neighbours (Neumann at the grid border).
fn relax(px: &mut [[f32; 3]], mask: &[bool], w: usize, h: usize, sweeps: usize, omega: f32) {
    for _ in 0..sweeps {
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if !mask[i] {
                    continue;
                }
                let (mut acc, mut n) = ([0.0f32; 3], 0.0f32);
                let mut add = |j: usize| {
                    for c in 0..3 {
                        acc[c] += px[j][c];
                    }
                    n += 1.0;
                };
                if x > 0 {
                    add(i - 1);
                }
                if x + 1 < w {
                    add(i + 1);
                }
                if y > 0 {
                    add(i - w);
                }
                if y + 1 < h {
                    add(i + w);
                }
                if n == 0.0 {
                    continue;
                }
                for c in 0..3 {
                    let target = acc[c] / n;
                    px[i][c] += omega * (target - px[i][c]);
                }
            }
        }
    }
}
