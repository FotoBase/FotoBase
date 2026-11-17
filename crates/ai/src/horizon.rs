//! Horizon-angle estimation for auto-level (TASK-07-007, PLAN §2 2026
//! additions "Auto-rotate / level horizon: AI-estimated rotation applied as
//! a recipe op").
//!
//! Heuristics only, no model: the horizon is the dominant near-horizontal
//! edge in a grayscale frame. Per column the strongest vertical-gradient
//! row is picked; a two-pass robust least-squares line fit through those
//! rows gives the tilt, and fit quality (inlier fraction × fit tightness ×
//! edge contrast) gives a confidence. Low confidence → `None`: the caller
//! must say "no confident horizon" instead of guessing (TASK-07-007
//! functional spec). A small ONNX model can later implement the same
//! interface.
//!
//! `angle_deg` is the leveling rotation to apply (the value for the
//! `auto_rotate` recipe op): the negative of the estimated tilt, in the
//! `straighten` convention (CW positive). Pure function over a pixel
//! buffer — no I/O, trivially testable; the caller decodes (never over
//! IPC, PLAN §3 rule 3).

/// One photo's horizon estimate.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct HorizonEstimate {
    /// Rotation (degrees, CW positive — the `straighten` convention) that
    /// levels the detected line. Always within ±45°.
    pub angle_deg: f32,
    /// 0..=1: how certain the line fit is (inlier fraction × fit tightness
    /// × edge contrast). Below [`MIN_CONFIDENCE`] the estimate is `None`.
    pub confidence: f32,
}

/// Below this the estimator declines instead of guessing (TASK-07-007:
/// "If no confident horizon is found, the action says so and does
/// nothing").
pub const MIN_CONFIDENCE: f32 = 0.35;

/// Estimate the horizon in an 8-bit grayscale frame. `width * height` must
/// cover `gray`. `None` when no confident near-horizontal horizon line is
/// found.
pub fn estimate_horizon(gray: &[u8], width: usize, height: usize) -> Option<HorizonEstimate> {
    let n = width.saturating_mul(height);
    if n == 0 || gray.len() < n || width < 8 || height < 8 {
        return None;
    }

    // Band where a horizon line plausibly sits: skip the outer margins so
    // frame borders and cropped-out edges don't seed the fit.
    let top = height / 8;
    let bottom = height * 7 / 8;

    // Per column: the strongest vertical-gradient row inside the band, with
    // a 5-row kernel so a soft (blurred/JPEG) horizon still registers its
    // full step. |Δy| detects a bright-above/dark-below step regardless of
    // polarity.
    let mut rows = Vec::with_capacity(width);
    let mut strengths = Vec::with_capacity(width);
    for x in 0..width {
        let mut best = 0i32;
        let mut best_y = top;
        for y in (top + 2)..(bottom - 2) {
            let d = i32::from(gray[(y + 2) * width + x]) - i32::from(gray[(y - 2) * width + x]);
            if d.abs() > best {
                best = d.abs();
                best_y = y;
            }
        }
        rows.push(best_y);
        strengths.push(best);
    }

    // Edge strength gate: a frame without any real edge has nothing to fit.
    let max_strength = *strengths.iter().max().unwrap_or(&0);
    if max_strength < 16 {
        return None;
    }

    // Least-squares line fit y = slope·x + intercept through (x, row(x)).
    let fit = |rows: &[usize]| -> (f64, f64) {
        let count = rows.len() as f64;
        let mean_x = (rows.len() - 1) as f64 / 2.0;
        let mean_y = rows.iter().map(|&r| r as f64).sum::<f64>() / count;
        let mut sxx = 0.0;
        let mut sxy = 0.0;
        for (x, &r) in rows.iter().enumerate() {
            let dx = x as f64 - mean_x;
            sxx += dx * dx;
            sxy += dx * (r as f64 - mean_y);
        }
        let slope = if sxx.abs() < 1e-9 { 0.0 } else { sxy / sxx };
        (slope, mean_y - slope * mean_x)
    };

    // Two-pass robust fit: fit all columns, drop outliers beyond 2σ of the
    // residual spread, refit on the survivors. A column whose strongest
    // edge is a roofline or a wave crest must not drag the fit.
    let (slope0, intercept0) = fit(&rows);
    let residuals: Vec<f64> = rows
        .iter()
        .enumerate()
        .map(|(x, &r)| (r as f64 - (slope0 * x as f64 + intercept0)).abs())
        .collect();
    let mean_res = residuals.iter().sum::<f64>() / residuals.len() as f64;
    let sigma = (residuals.iter().map(|d| (d - mean_res) * (d - mean_res)).sum::<f64>()
        / residuals.len() as f64)
        .sqrt();
    let keep: Vec<usize> = rows
        .iter()
        .zip(&residuals)
        .filter(|(_, d)| **d <= 2.0 * sigma.max(1.0))
        .map(|(r, _)| *r)
        .collect();
    if keep.len() < width / 4 {
        return None;
    }
    let (slope, intercept) = fit(&keep);

    // Inliers against the refit line: a real horizon hugs it within a few
    // rows; competing edges scatter.
    let mut inliers = 0usize;
    let mut total_sq = 0.0f64;
    for (x, &r) in rows.iter().enumerate() {
        let d = (r as f64 - (slope * x as f64 + intercept)).abs();
        if d <= 3.0 {
            inliers += 1;
            total_sq += d * d;
        }
    }
    let inlier_frac = inliers as f32 / width as f32;
    // RMS residual vs a few rows: a tight fit is confident, a loose one is
    // not — combined with the edge contrast (a faint edge is unreliable).
    let rms = (total_sq / inliers.max(1) as f64).sqrt() as f32;
    let contrast = (max_strength as f32 / 255.0).clamp(0.3, 1.0);
    let fit_quality = (1.0 - rms / 8.0).clamp(0.0, 1.0);
    // Edge uniqueness: how many rows per column carry a near-max gradient.
    // One horizon edge → a couple of rows (blur); a checkerboard or dense
    // texture → dozens of equal candidates, so "the strongest edge" is not
    // a line at all. Penalize ambiguity hard.
    let near_threshold = (max_strength as f32 * 0.9) as i32;
    let mut near_rows = 0usize;
    for x in 0..width {
        for y in (top + 2)..(bottom - 2) {
            let d = i32::from(gray[(y + 2) * width + x]) - i32::from(gray[(y - 2) * width + x]);
            if d.abs() >= near_threshold {
                near_rows += 1;
            }
        }
    }
    let avg_near = near_rows as f32 / width as f32;
    // A clean blurred horizon keeps 2–3 rows at near-max contrast; that is
    // the 1.0 anchor. Dense texture multiplies the count and drops out.
    let uniqueness = (2.5 / avg_near.max(1.0)).min(1.0);
    let confidence = (inlier_frac * 0.6 + fit_quality * 0.4) * contrast * uniqueness;
    if confidence < MIN_CONFIDENCE {
        return None;
    }

    // Leveling rotation = negative of the visual tilt. Pixel slope maps
    // straight to a visual angle (square pixels).
    let tilt_deg = slope.atan().to_degrees();
    let angle = (-tilt_deg).clamp(-45.0, 45.0) as f32;
    Some(HorizonEstimate { angle_deg: angle, confidence })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_small_frames_decline() {
        assert!(estimate_horizon(&[128; 64], 8, 8).is_none());
    }
}
