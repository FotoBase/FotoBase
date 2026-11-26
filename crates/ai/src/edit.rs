//! On-device AI editing runtimes (TASK-10-02-04, PLAN §7 F2 item 4):
//! EfficientSAM subject/sky segmentation, LaMa magic-erase inpainting, and
//! SCUNet AI denoising — all ONNX, CPU execution provider, no cloud calls.
//!
//! Same invariants as [`crate::runtime`]: plain in-memory RGB buffers in and
//! out (no image bytes over IPC, PLAN §3 rule 3), the shared no-telemetry
//! ONNX environment, and models that are opt-in, checksum-verified downloads
//! (the manifest entries in `crates/ai/model-manifest.json`).
//!
//! Preprocessing matches each model's reference usage exactly:
//! - EfficientSAM (labelme/yformer ONNX example): RGB [0, 1] float32 NCHW,
//!   no mean/std normalization; point prompts in encoder-input pixel
//!   coordinates (label 1 = include); decoder `orig_im_size` is the encoder
//!   input size; mask = decoder logits ≥ 0.
//! - LaMa (Carve-Photos ONNX port, big-lama): RGB [0, 1] float32 + binary
//!   mask, fixed 512×512 input, output already on the 0–255 scale.
//! - SCUNet (cszn, real-world PSNR checkpoint): RGB [0, 1] float32 NCHW,
//!   height/width divisible by 8 (symmetric reflection padding), output
//!   same layout and range.

use std::path::Path;

use ort::{
    session::builder::GraphOptimizationLevel,
    value::{TensorRef, TensorValueType},
};

use crate::runtime::{environment, RuntimeError};

fn build_session(path: &Path) -> Result<ort::session::Session, RuntimeError> {
    let mut builder = ort::session::Session::builder(environment())?;
    builder = builder
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| RuntimeError::Load { path: path.to_path_buf(), source: e.into() })?;
    builder
        .commit_from_file(path)
        .map_err(|e| RuntimeError::Load { path: path.to_path_buf(), source: e })
}

fn check_rgb(rgb: &[u8], width: u32, height: u32) -> Result<(), RuntimeError> {
    let expected = width as usize * height as usize * 3;
    if rgb.len() != expected {
        return Err(RuntimeError::InvalidInput(format!(
            "expected {expected} bytes for {width}x{height} RGB, got {}",
            rgb.len()
        )));
    }
    Ok(())
}

/// Extract output `i` as a flat f32 vec.
fn extract(outputs: &ort::session::SessionOutputs<'_>, i: usize) -> Result<Vec<f32>, RuntimeError> {
    let out = outputs[i].view().downcast::<TensorValueType<f32>>()?;
    let (_, data) = out.extract_tensor();
    Ok(data.to_vec())
}

/// Nearest-neighbor RGB8 resize (matching detail is not needed for the
/// mask/`mask`-resize paths; a box filter would soften mask edges).
fn resize_rgb_nearest(rgb: &[u8], w: u32, h: u32, out_w: u32, out_h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (out_w * out_h * 3) as usize];
    let (wu, hu) = (w as usize, h as usize);
    for oy in 0..out_h {
        let sy = (((oy as u64 + 1) * h as u64 / out_h as u64) as usize).min(hu - 1);
        for ox in 0..out_w {
            let sx = (((ox as u64 + 1) * w as u64 / out_w as u64) as usize).min(wu - 1);
            let src = (sy * wu + sx) * 3;
            let dst = ((oy as usize) * out_w as usize + ox as usize) * 3;
            out[dst..dst + 3].copy_from_slice(&rgb[src..src + 3]);
        }
    }
    out
}

/// Bilinear f32 single-channel resize (mask up/downsampling).
fn resize_mask_bilinear(mask: &[f32], w: u32, h: u32, out_w: u32, out_h: u32) -> Vec<f32> {
    let mut out = vec![0f32; (out_w * out_h) as usize];
    let (wu, hu) = (w as usize, h as usize);
    for oy in 0..out_h {
        let fy = (oy as f32 + 0.5) * h as f32 / out_h as f32 - 0.5;
        let y0 = (fy.floor().max(0.0) as usize).min(hu - 1);
        let y1 = (y0 + 1).min(hu - 1);
        let ty = (fy - y0 as f32).clamp(0.0, 1.0);
        for ox in 0..out_w {
            let fx = (ox as f32 + 0.5) * w as f32 / out_w as f32 - 0.5;
            let x0 = (fx.floor().max(0.0) as usize).min(wu - 1);
            let x1 = (x0 + 1).min(wu - 1);
            let tx = (fx - x0 as f32).clamp(0.0, 1.0);
            let a = mask[y0 * wu + x0];
            let b = mask[y0 * wu + x1];
            let c = mask[y1 * wu + x0];
            let d = mask[y1 * wu + x1];
            out[(oy as usize) * out_w as usize + ox as usize] =
                (a * (1.0 - tx) + b * tx) * (1.0 - ty) + (c * (1.0 - tx) + d * tx) * ty;
        }
    }
    out
}

/// Bilinear RGB8 resize (LaMa result back to the caller's resolution).
fn resize_rgb_bilinear(rgb: &[u8], w: u32, h: u32, out_w: u32, out_h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (out_w * out_h * 3) as usize];
    let (wu, hu) = (w as usize, h as usize);
    for oy in 0..out_h {
        let fy = (oy as f32 + 0.5) * h as f32 / out_h as f32 - 0.5;
        let y0 = (fy.floor().max(0.0) as usize).min(hu - 1);
        let y1 = (y0 + 1).min(hu - 1);
        let ty = (fy - y0 as f32).clamp(0.0, 1.0);
        for ox in 0..out_w {
            let fx = (ox as f32 + 0.5) * w as f32 / out_w as f32 - 0.5;
            let x0 = (fx.floor().max(0.0) as usize).min(wu - 1);
            let x1 = (x0 + 1).min(wu - 1);
            let tx = (fx - x0 as f32).clamp(0.0, 1.0);
            let dst = ((oy as usize) * out_w as usize + ox as usize) * 3;
            for c in 0..3 {
                let a = f32::from(rgb[(y0 * wu + x0) * 3 + c]);
                let b = f32::from(rgb[(y0 * wu + x1) * 3 + c]);
                let e = f32::from(rgb[(y1 * wu + x0) * 3 + c]);
                let d = f32::from(rgb[(y1 * wu + x1) * 3 + c]);
                out[dst + c] = ((a * (1.0 - tx) + b * tx) * (1.0 - ty)
                    + (e * (1.0 - tx) + d * tx) * ty)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// EfficientSAM (ti checkpoint) point-prompted segmentation: feed an image,
/// get a per-pixel subject/sky mask for local recipe adjustments
/// (TASK-10-02-04 functional spec).
pub struct SegmentationRuntime {
    encoder: ort::session::Session,
    decoder: ort::session::Session,
}

impl SegmentationRuntime {
    /// The encoder is capped at this edge length: feeding a 50 MP original
    /// would be both slow and pointless — the mask is a recipe *region*, so
    /// preview resolution is the right working size.
    const MAX_EDGE: u32 = 1024;

    pub fn load(encoder_path: &Path, decoder_path: &Path) -> Result<Self, RuntimeError> {
        Ok(Self { encoder: build_session(encoder_path)?, decoder: build_session(decoder_path)? })
    }

    /// Segment the region around the prompted points. `points` are in
    /// input-image pixel coordinates, `labels` 1.0 = include, 0.0 = exclude.
    /// Returns one f32 per pixel (0.0 outside, 1.0 inside) at the input
    /// resolution.
    pub fn mask_from_points(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
        points: &[(f32, f32)],
        labels: &[f32],
    ) -> Result<Vec<f32>, RuntimeError> {
        check_rgb(rgb, width, height)?;
        if points.is_empty() || points.len() != labels.len() {
            return Err(RuntimeError::InvalidInput(
                "points and labels must be non-empty and of equal length".into(),
            ));
        }
        // Scale to the working resolution; the point prompts scale with it.
        let scale = (Self::MAX_EDGE as f32 / width.max(height) as f32).min(1.0);
        let (ew, eh) =
            ((width as f32 * scale).round() as u32, (height as f32 * scale).round() as u32);
        let (ew, eh) = (ew.max(16), eh.max(16));
        let scaled = resize_rgb_nearest(rgb, width, height, ew, eh);
        let mut input = vec![0f32; (3 * ew * eh) as usize];
        let plane = (ew * eh) as usize;
        for (i, px) in scaled.as_chunks::<3>().0.iter().enumerate() {
            input[i] = px[0] as f32 / 255.0;
            input[plane + i] = px[1] as f32 / 255.0;
            input[2 * plane + i] = px[2] as f32 / 255.0;
        }
        let img = TensorRef::from_array_view(([1usize, 3, eh as usize, ew as usize], &*input))?;
        let embeddings = {
            let outputs = self.encoder.run(ort::inputs![img])?;
            extract(&outputs, 0)?
        };

        let n = points.len();
        let mut coords = vec![0f32; 2 * n];
        for (i, &(x, y)) in points.iter().enumerate() {
            coords[2 * i] = x * scale;
            coords[2 * i + 1] = y * scale;
        }
        let emb = TensorRef::from_array_view(([1usize, 1, n, 2usize], &*coords))?;
        let labels_v = labels.to_vec();
        let lab = TensorRef::from_array_view(([1usize, 1, n], &*labels_v))?;
        let orig = [eh as i64, ew as i64];
        let orig_t = TensorRef::from_array_view(([2usize], orig.as_slice()))?;
        let outputs = self.decoder.run(ort::inputs![
            "image_embeddings" => TensorRef::from_array_view(
                ([1usize, 256, 64, 64], &*embeddings))?,
            "batched_point_coords" => emb,
            "batched_point_labels" => lab,
            "orig_im_size" => orig_t,
        ])?;
        let logits = extract(&outputs, 0)?;
        // output_masks is [b, queries, 1, H, W]; one query per call. The
        // masks are already at the encoder input size (orig_im_size).
        let work = (eh * ew) as usize;
        let masks = if logits.len() == work {
            logits
        } else {
            logits.into_iter().take(work).collect::<Vec<_>>()
        };
        // logits ≥ 0 is the reference threshold (EfficientSAM_onnx_example).
        let binary: Vec<f32> =
            masks.into_iter().map(|v| if v >= 0.0 { 1.0 } else { 0.0 }).collect();
        Ok(resize_mask_bilinear(&binary, eh, ew, height, width)
            .into_iter()
            .map(|v| if v > 0.5 { 1.0 } else { 0.0 })
            .collect())
    }
}

/// LaMa (big-lama, Carve ONNX port) magic erase: fill a masked region from
/// its surroundings (TASK-10-02-04).
pub struct InpaintRuntime {
    model: ort::session::Session,
}

impl InpaintRuntime {
    /// The ONNX export is fixed at 512×512.
    const INPUT: u32 = 512;

    pub fn load(path: &Path) -> Result<Self, RuntimeError> {
        Ok(Self { model: build_session(path)? })
    }

    /// Fill every masked pixel (mask > 0.5) from the surrounding content.
    /// Returns an RGB8 buffer the same size as the input where unmasked
    /// pixels are bit-identical to the input — inpainting must not alter
    /// content outside the mask.
    pub fn inpaint(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
        mask: &[f32],
    ) -> Result<Vec<u8>, RuntimeError> {
        check_rgb(rgb, width, height)?;
        if mask.len() != (width * height) as usize {
            return Err(RuntimeError::InvalidInput("mask must have one value per pixel".into()));
        }
        let n = Self::INPUT;
        let scaled = resize_rgb_nearest(rgb, width, height, n, n);
        let coarse = resize_mask_bilinear(mask, width, height, n, n);
        let mut image = vec![0f32; (3 * n * n) as usize];
        let plane = (n * n) as usize;
        for (i, px) in scaled.as_chunks::<3>().0.iter().enumerate() {
            image[i] = px[0] as f32 / 255.0;
            image[plane + i] = px[1] as f32 / 255.0;
            image[2 * plane + i] = px[2] as f32 / 255.0;
        }
        let mask_in: Vec<f32> = coarse.iter().map(|&m| if m > 0.5 { 1.0 } else { 0.0 }).collect();
        let img = TensorRef::from_array_view(([1usize, 3, n as usize, n as usize], &*image))?;
        let msk = TensorRef::from_array_view(([1usize, 1, n as usize, n as usize], &*mask_in))?;
        let outputs = self.model.run(ort::inputs![img, msk])?;
        let out = extract(&outputs, 0)?;
        // The ONNX port emits the filled image on the 0–255 scale (the
        // reference demo casts the output straight to uint8).
        let mut filled = vec![0u8; (3 * n * n) as usize];
        for (dst, &v) in filled.iter_mut().zip(&out) {
            *dst = v.round().clamp(0.0, 255.0) as u8;
        }
        let back = resize_rgb_bilinear(&filled, n, n, width, height);
        let mut result = rgb.to_vec();
        for (i, &m) in mask.iter().enumerate() {
            if m > 0.5 {
                result[i * 3..i * 3 + 3].copy_from_slice(&back[i * 3..i * 3 + 3]);
            }
        }
        Ok(result)
    }
}

/// SCUNet (real-world PSNR checkpoint) blind denoising for high-ISO photos
/// (TASK-10-02-04).
pub struct DenoiseRuntime {
    model: ort::session::Session,
}

impl DenoiseRuntime {
    pub fn load(path: &Path) -> Result<Self, RuntimeError> {
        Ok(Self { model: build_session(path)? })
    }

    /// Denoise an RGB8 buffer. The model needs height/width divisible by 8;
    /// the buffer is symmetric-padded, denoised, and cropped back.
    pub fn denoise(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, RuntimeError> {
        check_rgb(rgb, width, height)?;
        let pw = width.div_ceil(8) * 8;
        let ph = height.div_ceil(8) * 8;
        let padded = pad_symmetric(rgb, width, height, pw, ph);
        let mut input = vec![0f32; (3 * pw * ph) as usize];
        let plane = (pw * ph) as usize;
        for (i, px) in padded.as_chunks::<3>().0.iter().enumerate() {
            input[i] = px[0] as f32 / 255.0;
            input[plane + i] = px[1] as f32 / 255.0;
            input[2 * plane + i] = px[2] as f32 / 255.0;
        }
        let img = TensorRef::from_array_view(([1usize, 3, ph as usize, pw as usize], &*input))?;
        let outputs = self.model.run(ort::inputs![img])?;
        let out = extract(&outputs, 0)?;
        let mut result = vec![0u8; (width * height * 3) as usize];
        for y in 0..height {
            for x in 0..width {
                let si = (y as usize) * pw as usize + x as usize;
                let di = ((y * width + x) * 3) as usize;
                for c in 0..3 {
                    let v = out[c * plane + si].clamp(0.0, 1.0) * 255.0;
                    result[di + c] = v.round() as u8;
                }
            }
        }
        Ok(result)
    }
}

/// Symmetric (reflect-without-edge-repeat) padding, matching the LaMa
/// `np.pad(..., mode="symmetric")` convention used by the reference
/// preprocessing.
fn pad_symmetric(rgb: &[u8], w: u32, h: u32, pw: u32, ph: u32) -> Vec<u8> {
    if pw == w && ph == h {
        return rgb.to_vec();
    }
    let (wu, hu) = (w as usize, h as usize);
    let reflect = |i: i64, len: usize| -> usize {
        let len = len as i64;
        let mut i = i.rem_euclid(2 * len);
        if i >= len {
            i = 2 * len - 1 - i;
        }
        i.clamp(0, len - 1) as usize
    };
    let mut out = vec![0u8; (pw * ph * 3) as usize];
    for y in 0..ph {
        let sy = reflect(y as i64, hu);
        for x in 0..pw {
            let sx = reflect(x as i64, wu);
            let src = (sy * wu + sx) * 3;
            let dst = ((y * pw + x) * 3) as usize;
            out[dst..dst + 3].copy_from_slice(&rgb[src..src + 3]);
        }
    }
    out
}
