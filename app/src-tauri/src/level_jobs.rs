//! The auto-level horizon estimate as a job-queue job (TASK-07-007, PLAN
//! §3 rule 4): a user-invoked "Auto-level" decodes the photo in the bounded
//! queue — never on the UI thread — and returns a single angle ± confidence
//! or `None`. Nothing is written: the estimate only seeds a recipe op the
//! user applies (the op itself is the non-destructive path). No image bytes
//! cross IPC (PLAN §3 rule 3).

pub use fotobase_ai::HorizonEstimate;

/// The horizon-estimation backend. Pure-math production engine below; a
/// fake stands in for tests, and a small ONNX horizon model can implement
/// the same interface later (TASK-07-007 technical spec).
pub trait HorizonEngine: Send {
    fn estimate(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Option<HorizonEstimate>, String>;
}

/// Production engine: the ai crate's edge/orientation-based line detection
/// (no model needed).
pub struct PureHorizonEngine;

impl HorizonEngine for PureHorizonEngine {
    fn estimate(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Option<HorizonEstimate>, String> {
        let gray = fotobase_ai::gray_from_rgb(rgb, width as usize, height as usize);
        // Downscale to a bounded working size: the estimator is O(w·h) per
        // pass and a 50 MP frame gains nothing from full resolution.
        let (w, h) = (width as usize, height as usize);
        let max_edge = 512usize;
        let step = w.max(h).div_ceil(max_edge).max(1);
        if step == 1 {
            return Ok(fotobase_ai::estimate_horizon(&gray, w, h));
        }
        let dw = w / step;
        let dh = h / step;
        if dw < 8 || dh < 8 {
            return Ok(fotobase_ai::estimate_horizon(&gray, w, h));
        }
        let mut small = Vec::with_capacity(dw * dh);
        for y in 0..dh {
            let sy = y * step;
            for x in 0..dw {
                small.push(gray[sy * w + x * step]);
            }
        }
        Ok(fotobase_ai::estimate_horizon(&small, dw, dh))
    }
}

/// Estimate the horizon of one photo on disk. Decode errors surface as
/// `Err` — the command reports them instead of guessing.
pub fn estimate_for_path(
    path: &std::path::Path,
    engine: &mut dyn HorizonEngine,
) -> Result<Option<HorizonEstimate>, String> {
    let (rgb, w, h) =
        fotobase_render::decode_rgb(path).map_err(|e| format!("decode {}: {e}", path.display()))?;
    engine.estimate(&rgb, w, h)
}
