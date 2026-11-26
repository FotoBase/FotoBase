//! The AI editing backend (TASK-10-02-04, PLAN §7 F2 item 4): `ai_erase`
//! (LaMa inpainting) and `ai_denoise` (SCUNet) as recipe-op backends behind
//! the `OpBackend` seam — the same seam as Lensfun. The runtimes live in
//! `fotobase-ai`; this module only adapts recipe ops to them.
//!
//! Constructed only when the opt-in models are installed (PLAN §1: models
//! are checksum-verified downloads the user approved). `apply` returns
//! `None` for ops it cannot run, so recipes render without the AI step
//! (skip-and-warn, spec §2) rather than failing.
//!
//! Segmenter masks never appear here: the editor runs EfficientSAM
//! interactively and stores the resulting polygon as the op's `mask` region
//! — the recipe stays a pure, portable description.

use fotobase_core::apply::OpBackend;
use fotobase_core::buffer::LinearImage;
use fotobase_core::recipe::Op;
use serde_json::Value;

use fotobase_ai::{DenoiseRuntime, InpaintRuntime};

/// Mask-region op types this backend implements.
const AI_OPS: &[&str] = &["ai_erase", "ai_denoise"];

pub struct AiBackend {
    // `ort` sessions need `&mut` to run; the backend is shared behind `&`,
    // so the sessions sit behind a mutex (recipe application is serial).
    inpaint: Option<std::sync::Mutex<InpaintRuntime>>,
    denoise: Option<std::sync::Mutex<DenoiseRuntime>>,
}

impl AiBackend {
    /// Load whatever AI editing models are installed (and checksum-valid)
    /// under `models_dir`. Missing models simply disable their op.
    pub fn from_models_dir(models_dir: &std::path::Path) -> Self {
        let registry = fotobase_ai::ModelRegistry::from_manifest_bytes(
            fotobase_ai::MODEL_MANIFEST_JSON.as_bytes(),
        )
        .ok();
        let installed = |name: &str| -> bool {
            registry
                .as_ref()
                .and_then(|r| r.entry(name).map(|e| r.is_installed(e, models_dir)))
                .unwrap_or(false)
        };
        let inpaint = installed("lama_fp32.onnx")
            .then(|| InpaintRuntime::load(&models_dir.join("lama_fp32.onnx")).ok())
            .flatten()
            .map(std::sync::Mutex::new);
        let denoise = installed("scunet_color_real_psnr.onnx")
            .then(|| DenoiseRuntime::load(&models_dir.join("scunet_color_real_psnr.onnx")).ok())
            .flatten()
            .map(std::sync::Mutex::new);
        Self { inpaint, denoise }
    }

    /// Constructor for tests with explicit paths (no manifest round-trip).
    pub fn from_paths(
        inpaint: Option<std::path::PathBuf>,
        denoise: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            inpaint: inpaint.and_then(|p| InpaintRuntime::load(&p).ok()).map(std::sync::Mutex::new),
            denoise: denoise.and_then(|p| DenoiseRuntime::load(&p).ok()).map(std::sync::Mutex::new),
        }
    }
}

impl OpBackend for AiBackend {
    fn apply(&self, img: &LinearImage, op: &Op) -> Option<LinearImage> {
        if !AI_OPS.contains(&op.op_type.as_str()) {
            return None;
        }
        let amount = fotobase_core::apply::op_fnum(op, "amount", 1.0) as f32;
        match op.op_type.as_str() {
            "ai_erase" => {
                let inpaint = self.inpaint.as_ref()?;
                let mut inpaint = inpaint.lock().ok()?;
                let points = op
                    .params
                    .as_ref()
                    .and_then(|p| p.get("mask"))
                    .and_then(Value::as_array)
                    .map(|v| v.as_slice())?;
                let feather = fotobase_core::apply::op_fnum(op, "feather", 0.02) as f64;
                let mask =
                    fotobase_core::apply::polygon_mask(img.width, img.height, points, feather);
                let rgb = to_u8_srgb(img);
                let filled = inpaint.inpaint(&rgb, img.width, img.height, &mask).ok()?;
                Some(from_u8_srgb(&filled, img.width, img.height))
            }
            "ai_denoise" => {
                if amount <= 0.0 {
                    // Strength 0 is a no-op: bit-identical buffer, like the
                    // other strength-scaled ops (TASK-04-009 semantics).
                    return Some(img.clone());
                }
                let denoise = self.denoise.as_ref()?;
                let mut denoise = denoise.lock().ok()?;
                let rgb = to_u8_srgb(img);
                let out = denoise.denoise(&rgb, img.width, img.height).ok()?;
                let denoised = from_u8_srgb(&out, img.width, img.height);
                let mut blended = img.clone();
                for (dst, src) in blended.data.iter_mut().zip(denoised.data.iter()) {
                    *dst = *dst + (*src - *dst) * amount;
                }
                Some(blended)
            }
            _ => None,
        }
    }
}

/// sRGB transfer (encode): linear-light [0, 1] → display-encoded [0, 1].
fn srgb_encode(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB transfer (decode): display-encoded [0, 1] → linear-light [0, 1].
fn srgb_decode(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn to_u8_srgb(img: &LinearImage) -> Vec<u8> {
    img.data.iter().map(|&v| (srgb_encode(v.clamp(0.0, 1.0)) * 255.0).round() as u8).collect()
}

fn from_u8_srgb(rgb: &[u8], width: u32, height: u32) -> LinearImage {
    LinearImage {
        width,
        height,
        data: rgb.iter().map(|&b| srgb_decode(f32::from(b) / 255.0)).collect(),
    }
}
