//! TASK-07-006: the cheap sharpness/quality score (Laplacian-variance
//! sharpness + exposure heuristics — no model). Pure functions over a pixel
//! buffer, so every rule is testable without ONNX.

use fotobase_ai::{gray_from_rgb, quality_score};

/// A high-contrast checkerboard: maximal local gradient, sharp by
/// construction.
fn checkerboard(width: usize, height: usize) -> Vec<u8> {
    let mut gray = vec![128u8; width * height];
    for y in 0..height {
        for x in 0..width {
            if (x / 4 + y / 4) % 2 == 0 {
                gray[y * width + x] = 220;
            } else {
                gray[y * width + x] = 30;
            }
        }
    }
    gray
}

/// Box-average `gray` into `out` with a 3×3 kernel: a cheap stand-in for an
/// out-of-focus frame.
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
fn a_sharp_frame_scores_above_a_blurred_one() {
    let sharp = checkerboard(64, 64);
    let blurred = blur3(&sharp, 64, 64);
    let sharp_score = quality_score(&sharp, 64, 64);
    let blurred_score = quality_score(&blurred, 64, 64);
    assert!(
        sharp_score.total > blurred_score.total,
        "sharp {} must outrank blurred {}",
        sharp_score.total,
        blurred_score.total
    );
    assert!(sharp_score.sharpness > blurred_score.sharpness);
}

#[test]
fn a_flat_frame_has_no_sharpness() {
    let flat = vec![128u8; 32 * 32];
    let s = quality_score(&flat, 32, 32);
    assert_eq!(s.sharpness, 0.0);
    assert_eq!(s.total, 0.0);
    // Perfectly mid-gray wastes no light: exposure stays high.
    assert!(s.exposure > 0.9, "exposure {}", s.exposure);
}

#[test]
fn a_clipped_frame_loses_exposure() {
    // Same checkerboard structure, but every pixel slammed to white: the
    // highlight detail is gone, so exposure must drop well below mid-gray.
    let mut blown = checkerboard(64, 64);
    for p in blown.iter_mut() {
        *p = if *p > 128 { 255 } else { 250 };
    }
    let blown = quality_score(&blown, 64, 64);
    let mid = quality_score(&vec![128u8; 64 * 64], 64, 64);
    assert!(blown.exposure < mid.exposure, "blown {} mid {}", blown.exposure, mid.exposure);
}

#[test]
fn gray_from_rgb_uses_rec601_luma() {
    let rgb = [0, 0, 0, 255, 255, 255, 76, 150, 30];
    let gray = gray_from_rgb(&rgb, 3, 1);
    assert_eq!(gray, vec![0, 255, 114]);
}

#[test]
fn scores_are_finite_and_non_negative_for_extremes() {
    for gray in [vec![0u8; 16 * 16], vec![255u8; 16 * 16], checkerboard(16, 16)] {
        let s = quality_score(&gray, 16, 16);
        assert!(s.total.is_finite() && s.total >= 0.0);
        assert!(s.sharpness.is_finite() && s.sharpness >= 0.0);
        assert!((0.0..=1.0).contains(&s.exposure));
    }
}
