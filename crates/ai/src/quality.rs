//! Cheap sharpness/quality scoring for burst culling (TASK-07-006, PLAN §6
//! P3 "AI culling: burst grouping ranked by sharpness/quality to pick the
//! best shot").
//!
//! Heuristics only, no model: Laplacian-variance sharpness plus an exposure
//! penalty for clipped shadows/highlights, computed over the decoded preview.
//! A learned quality model can later implement the same `quality_score`
//! interface without changing any caller.
//!
//! Pure function over a pixel buffer — no I/O, no runtime, trivially
//! testable. The caller decodes (never over IPC, PLAN §3 rule 3).

/// One photo's quality assessment. `total` is what burst ranking uses;
/// the components are kept for diagnostics and future UI display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityScore {
    /// Laplacian variance: high for detailed, in-focus frames; 0 for flat.
    pub sharpness: f32,
    /// Exposure suitability in 0..=1: penalizes clipped shadows and
    /// highlights (lost detail).
    pub exposure: f32,
    /// The ranking score: sharpness scaled by the exposure factor.
    pub total: f32,
}

/// Rec601 luma from interleaved RGB8 (`rgb.len() == width * height * 3`).
pub fn gray_from_rgb(rgb: &[u8], width: usize, height: usize) -> Vec<u8> {
    rgb.as_chunks::<3>()
        .0
        .iter()
        .take(width * height)
        .map(|px| {
            let luma =
                0.299 * f32::from(px[0]) + 0.587 * f32::from(px[1]) + 0.114 * f32::from(px[2]);
            luma.round().clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// The 3×3 Laplacian magnitude at one pixel (0 at the borders).
fn laplacian(gray: &[u8], width: usize, height: usize, x: usize, y: usize) -> i32 {
    if x == 0 || y == 0 || x + 1 >= width || y + 1 >= height {
        return 0;
    }
    let at = |xx: usize, yy: usize| i32::from(gray[yy * width + xx]);
    let c = at(x, y);
    at(x - 1, y) + at(x + 1, y) + at(x, y - 1) + at(x, y + 1) - 4 * c
}

/// Score one 8-bit grayscale frame. `width * height` must cover `gray`.
pub fn quality_score(gray: &[u8], width: usize, height: usize) -> QualityScore {
    let n = width.saturating_mul(height);
    if n == 0 || gray.len() < n {
        return QualityScore { sharpness: 0.0, exposure: 0.0, total: 0.0 };
    }

    // Laplacian variance (TASK-07-006: the standard cheap sharpness proxy).
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    for y in 0..height {
        for x in 0..width {
            let l = f64::from(laplacian(gray, width, height, x, y));
            sum += l;
            sum_sq += l * l;
        }
    }
    let mean = sum / n as f64;
    let sharpness = (sum_sq / n as f64 - mean * mean) as f32;

    // Exposure: the fraction of pixels with lost detail. A little clipping
    // is normal (speculars, silhouettes); beyond ~a third of the frame the
    // shot is unusable as "the best".
    let clipped = gray.iter().filter(|&&p| p <= 8 || p >= 247).count();
    let lost = clipped as f32 / n as f32;
    let exposure = (1.0 - lost * 2.0).clamp(0.0, 1.0);

    QualityScore { sharpness, exposure, total: sharpness * exposure }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gray_conversion_rounds_rec601() {
        assert_eq!(gray_from_rgb(&[76, 150, 30], 1, 1), vec![114]);
    }
}
