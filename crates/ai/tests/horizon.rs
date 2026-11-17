//! TASK-07-007: horizon estimation for auto-rotate (PLAN §2 2026 additions
//! "Auto-rotate / level horizon"). Pure functions over a pixel buffer — no
//! model, no I/O, trivially testable.

use fotobase_ai::estimate_horizon;

/// One horizontal edge row `h` at fraction `y` of the (downscaled) height,
/// brighter sky above, darker ground below, blurred with a 3×3 box so the
/// edge is not a single-pixel knife.
fn horizon_image(width: usize, height: usize, y_frac: f64) -> Vec<u8> {
    let y_edge = (y_frac * height as f64) as usize;
    let mut gray = vec![90u8; width * height];
    for yy in 0..height {
        for xx in 0..width {
            gray[yy * width + xx] = if yy < y_edge { 200 } else { 60 };
        }
    }
    blur3(&gray, width, height)
}

/// The same image, sheared so the horizon line has pixel slope `slope`
/// (positive = down to the right = clockwise tilt).
fn tilted(width: usize, height: usize, y_frac: f64, slope: f64) -> Vec<u8> {
    let mut gray = vec![90u8; width * height];
    for yy in 0..height {
        for xx in 0..width {
            let xf = xx as f64;
            let y_edge = y_frac * height as f64 + slope * (xf - width as f64 / 2.0);
            gray[yy * width + xx] = if (yy as f64) < y_edge { 200 } else { 60 };
        }
    }
    blur3(&gray, width, height)
}

/// Box-average with a 3×3 kernel (same stand-in as the quality tests).
fn blur3(gray: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut out = gray.to_vec();
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let mut sum = 0u32;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    sum += gray[((y as i32 + dy) * width as i32 + x as i32 + dx) as usize] as u32;
                }
            }
            out[y * width + x] = (sum / 9) as u8;
        }
    }
    out
}

#[test]
fn a_known_tilt_is_estimated_within_tolerance() {
    // 5° clockwise tilt: tan(5°) ≈ 0.0875 pixel slope.
    let slope = (5.0f64).to_radians().tan();
    let img = tilted(320, 240, 0.5, slope);
    let est = estimate_horizon(&img, 320, 240).expect("a clear horizon must be found");
    assert!(
        est.confidence > 0.5,
        "a clear horizon needs a confident estimate, got {}",
        est.confidence
    );
    // The rotation that levels the frame is the negative tilt.
    let leveling = -slope.atan().to_degrees();
    assert!(
        (est.angle_deg as f64 - leveling).abs() < 1.0,
        "estimated {} must level a 5° tilt (expected {leveling:.2})",
        est.angle_deg
    );
}

#[test]
fn a_level_horizon_estimates_near_zero() {
    let img = horizon_image(320, 240, 0.5);
    let est = estimate_horizon(&img, 320, 240).expect("a clear horizon must be found");
    assert!(est.angle_deg.abs() < 0.5, "level horizon, got {}", est.angle_deg);
    assert!(est.confidence > 0.5);
}

#[test]
fn a_flat_cloudy_sky_reports_low_confidence() {
    // Smooth vertical gradient, no edge anywhere: no horizon to find.
    let mut gray = Vec::with_capacity(320 * 240);
    for y in 0..240 {
        for _x in 0..320 {
            gray.push((90 + y / 4) as u8);
        }
    }
    assert!(estimate_horizon(&gray, 320, 240).is_none(), "no horizon must not guess one");
}

#[test]
fn a_high_contrast_scene_without_a_dominant_line_reports_low_confidence() {
    // Many edges at many angles (checkerboard): no single dominant
    // near-horizontal line, so the estimator must decline.
    let mut gray = vec![128u8; 320 * 240];
    for y in 0..240 {
        for x in 0..320 {
            if (x / 8 + y / 8) % 2 == 0 {
                gray[y * 320 + x] = 220;
            } else {
                gray[y * 320 + x] = 30;
            }
        }
    }
    assert!(estimate_horizon(&gray, 320, 240).is_none());
}

#[test]
fn degenerate_inputs_decline() {
    assert!(estimate_horizon(&[], 0, 0).is_none());
    assert!(estimate_horizon(&[128], 1, 1).is_none());
}
