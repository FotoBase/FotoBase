//! Integration tests for the on-device AI editing runtimes (TASK-10-02-04,
//! PLAN §7 F2 item 4): EfficientSAM subject/sky masks, LaMa magic erase,
//! SCUNet AI denoise.
//!
//! Gated on real models in `target/ai-models/` (populated by
//! `tools/download_models.sh`), same pattern as `runtime.rs`: the
//! manifest/checksum gate runs everywhere; the model-execution leg runs
//! only where the weights are present, keeping `cargo test --workspace`
//! green on machines without the ~340 MB of editing weights.

use std::path::PathBuf;

fn models_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ai-models");
    let enc = dir.join("efficientsam_ti_encoder.onnx");
    let dec = dir.join("efficientsam_ti_decoder.onnx");
    if enc.is_file() && dec.is_file() {
        Some(dir)
    } else {
        None
    }
}

fn lama_path() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ai-models");
    let p = dir.join("lama_fp32.onnx");
    p.is_file().then_some(p)
}

fn denoise_path() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ai-models");
    let p = dir.join("scunet_color_real_psnr.onnx");
    p.is_file().then_some(p)
}

/// A small synthetic scene: bright sky above the horizon line, textured
/// ground below — enough contrast for the segmenter to be exercised on a
/// real subject/sky split.
fn scene(w: u32, h: u32) -> Vec<u8> {
    let mut rgb = vec![0u8; (w * h * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let sky = y < h / 2;
            let i = ((y * w + x) * 3) as usize;
            if sky {
                let v = 180 + (x % 32) as u8;
                rgb[i] = v;
                rgb[i + 1] = 200;
                rgb[i + 2] = 240;
            } else {
                let v = (30 + ((x * 7 + y * 13) % 48)) as u8;
                rgb[i] = v;
                rgb[i + 1] = v + 20;
                rgb[i + 2] = v / 2;
            }
        }
    }
    rgb
}

#[test]
fn segmenter_masks_a_point_prompted_region() {
    let Some(dir) = models_dir() else {
        eprintln!("skipping: EfficientSAM models not downloaded into target/ai-models");
        return;
    };
    let mut rt = fotobase_ai::SegmentationRuntime::load(
        &dir.join("efficientsam_ti_encoder.onnx"),
        &dir.join("efficientsam_ti_decoder.onnx"),
    )
    .expect("segmenter loads on CPU");

    let (w, h) = (256u32, 256u32);
    let rgb = scene(w, h);
    // One include point in the sky half at the image center-top.
    let mask = rt
        .mask_from_points(&rgb, w, h, &[(w as f32 / 2.0, h as f32 * 0.25)], &[1.0])
        .expect("segmentation runs");

    assert_eq!(mask.len(), (w * h) as usize, "mask is one value per pixel");
    assert!(
        mask.iter().any(|&m| m > 0.5) && mask.iter().any(|&m| m <= 0.5),
        "mask must be selective (some in, some out)"
    );
    // The prompted sky half must be mostly inside the mask.
    let sky_in = mask[..(w * h / 2) as usize].iter().filter(|&&m| m > 0.5).count();
    assert!(
        sky_in as f32 / (w * h / 2) as f32 > 0.5,
        "a sky point should cover most of the sky region ({sky_in} in)"
    );
}

#[test]
fn segmenter_masks_a_sky_point_differently_from_a_ground_point() {
    let Some(dir) = models_dir() else {
        eprintln!("skipping: EfficientSAM models not downloaded into target/ai-models");
        return;
    };
    let mut rt = fotobase_ai::SegmentationRuntime::load(
        &dir.join("efficientsam_ti_encoder.onnx"),
        &dir.join("efficientsam_ti_decoder.onnx"),
    )
    .expect("segmenter loads on CPU");
    let (w, h) = (256u32, 256u32);
    let rgb = scene(w, h);
    let sky =
        rt.mask_from_points(&rgb, w, h, &[(w as f32 / 2.0, h as f32 * 0.25)], &[1.0]).unwrap();
    let ground =
        rt.mask_from_points(&rgb, w, h, &[(w as f32 / 2.0, h as f32 * 0.75)], &[1.0]).unwrap();
    let overlap: f32 = sky.iter().zip(&ground).filter(|(a, b)| **a > 0.5 && **b > 0.5).count()
        as f32
        / (w * h) as f32;
    assert!(
        overlap < 0.5,
        "sky and ground prompts must select different regions (overlap {overlap})"
    );
}

#[test]
fn inpainter_fills_a_masked_hole_from_its_surroundings() {
    let Some(path) = lama_path() else {
        eprintln!("skipping: LaMa model not downloaded into target/ai-models");
        return;
    };
    let mut rt = fotobase_ai::InpaintRuntime::load(&path).expect("LaMa loads on CPU");

    // A flat mid-gray field with a white square strictly inside the mask
    // (the mask extends past the white area, so no unmasked white edge
    // pixels survive the composite).
    let (w, h) = (256u32, 256u32);
    let mut rgb = vec![128u8; (w * h * 3) as usize];
    let mut mask = vec![0f32; (w * h) as usize];
    for y in 96..160 {
        for x in 96..160 {
            mask[(y * w + x) as usize] = 1.0;
        }
    }
    for y in 112..144 {
        for x in 112..144 {
            let i = ((y * w + x) * 3) as usize;
            rgb[i] = 255;
            rgb[i + 1] = 255;
            rgb[i + 2] = 255;
        }
    }
    let out = rt.inpaint(&rgb, w, h, &mask).expect("inpaint runs");
    assert_eq!(out.len(), (w * h * 3) as usize);
    // The hole must be filled with something close to the surrounding gray,
    // not left white.
    let mut worst = 0u32;
    for y in 116..140 {
        for x in 116..140 {
            let i = ((y * w + x) * 3) as usize;
            worst = worst.max((out[i] as i32 - 128).unsigned_abs());
        }
    }
    assert!(
        worst < 60,
        "hole center must be filled near the background, worst channel gap {worst}"
    );
    // Unmasked pixels are returned unchanged (originals read-only in spirit:
    // inpainting must not alter content outside the mask).
    for y in [0usize, 255] {
        for x in [0usize, 255] {
            let i = (y * w as usize + x) * 3;
            assert_eq!(out[i], 128, "unmasked pixel must be untouched");
        }
    }
}

#[test]
fn denoiser_removes_synthetic_grain_from_a_flat_field() {
    let Some(path) = denoise_path() else {
        eprintln!("skipping: SCUNet model not downloaded into target/ai-models");
        return;
    };
    let mut rt = fotobase_ai::DenoiseRuntime::load(&path).expect("denoiser loads on CPU");

    // Flat gray + deterministic pseudo-noise (LCG), σ ≈ 25/255·? — plainly
    // visible grain on a flat field.
    let (w, h) = (128u32, 128u32);
    let mut rgb = vec![0u8; (w * h * 3) as usize];
    let mut state = 42u64;
    for chunk in rgb.chunks_mut(3) {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let noise = ((state >> 33) % 51) as i32 - 25;
        let v = (128 + noise).clamp(0, 255) as u8;
        chunk[0] = v;
        chunk[1] = v;
        chunk[2] = v;
    }
    let out = rt.denoise(&rgb, w, h).expect("denoise runs");
    assert_eq!(out.len(), rgb.len());

    let noise_energy = |buf: &[u8]| -> f64 {
        buf.chunks(3).map(|c| (c[0] as f64 - 128.0).powi(2)).sum::<f64>() / (w * h) as f64
    };
    let before = noise_energy(&rgb);
    let after = noise_energy(&out);
    assert!(
        after < 0.25 * before,
        "denoiser must substantially reduce grain (before RMS² {before:.1}, after {after:.1})"
    );
    // Mean level must be preserved (a denoiser that shifts brightness is
    // not "usable on high-ISO photos").
    let mean = |buf: &[u8]| buf.chunks(3).map(|c| c[0] as f64).sum::<f64>() / (w * h) as f64;
    assert!((mean(&out) - mean(&rgb)).abs() < 12.0, "mean brightness must be roughly preserved");
}
