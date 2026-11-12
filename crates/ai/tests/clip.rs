//! TASK-07-001 TDD: the CLIP ViT-B/32 runtime. The ONNX weights are an
//! opt-in 350 MB download — never present in CI — so the unit tests cover
//! what runs without the model: the manifest entry (license gate) and the
//! preprocessing math. The inference path is exercised by the fake-engine
//! pass tests in `app/src-tauri` plus the perf-acceptance harness, the
//! same split the face pipeline (TASK-05-002) uses.

use fotobase_ai::clip::{clip_preprocess, CLIP_EMBEDDING_DIM, CLIP_INPUT};
use fotobase_ai::ModelRegistry;

fn manifest_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("model-manifest.json")
}

/// The CLIP image tower is pinned in the manifest with permissive (MIT)
/// weights, SHA-256 + size, and an https URL — the CI license gate
/// (tools/verify_model_licenses.py) enforces the same allowlist.
#[test]
fn clip_model_is_manifested_permissive_and_pinned() {
    let registry = ModelRegistry::from_manifest_file(&manifest_path()).expect("manifest parses");
    let entry = registry
        .entries()
        .iter()
        .find(|e| e.name == "open_clip_vit_b32_laion2b_visual.onnx")
        .expect("CLIP visual tower pinned in the manifest");
    assert_eq!(entry.license, "MIT", "laion2b ViT-B/32 weights are MIT");
    assert!(entry.url.starts_with("https://"));
    assert_eq!(entry.sha256.len(), 64);
    // The real ViT-B/32 fp32 visual tower is ~350 MB; a wrong pin would
    // break the size gate on download.
    assert!(entry.size_bytes > 300_000_000, "size pin looks wrong");
}

/// Preprocessing matches OpenCLIP's reference transform: 224×224 NCHW,
/// /255, then CLIP mean/std normalization per channel.
#[test]
fn preprocess_resizes_to_224_nchw_with_clip_normalization() {
    let w = 4u32;
    let h = 4u32;
    // Uniform mid-gray image: every normalized value must be equal.
    let rgb = vec![128u8; (w * h * 3) as usize];
    let out = clip_preprocess(&rgb, w, h);
    assert_eq!(out.len(), (3 * CLIP_INPUT * CLIP_INPUT) as usize);
    let plane = (CLIP_INPUT * CLIP_INPUT) as usize;
    // 128/255 = 0.50196; channel means/stds from OpenCLIP.
    // Same reference constants as the implementation (clippy's
    // excessive-precision lint would truncate them here, too).
    #[allow(clippy::excessive_precision)]
    let want: [f64; 3] = [
        (128.0 / 255.0 - 0.48145466) / 0.26862954,
        (128.0 / 255.0 - 0.4578275) / 0.26130258,
        (128.0 / 255.0 - 0.40821073) / 0.27577711,
    ];
    for c in 0..3 {
        for i in 0..plane {
            assert!(
                (out[c * plane + i] - want[c] as f32).abs() < 1e-5,
                "channel {c} pixel {i}: {} vs {}",
                out[c * plane + i],
                want[c]
            );
        }
    }
}

/// The tensor layout is channel-planar (NCHW): the R plane is the first
/// CLIP_INPUT² floats, all R values.
#[test]
fn preprocess_is_planar_not_interleaved() {
    let w = 2u32;
    let h = 2u32;
    let rgb = vec![255u8, 0, 0, 255u8, 0, 0, 255u8, 0, 0, 255u8, 0, 0];
    let out = clip_preprocess(&rgb, w, h);
    let plane = (CLIP_INPUT * CLIP_INPUT) as usize;
    // All R pixels identical → plane 0 constant; G/B planes constant too.
    for c in 0..3 {
        let first = out[c * plane];
        for i in 1..plane {
            assert!((out[c * plane + i] - first).abs() < 1e-6, "plane {c} not uniform");
        }
    }
    assert!(out[0] > out[plane], "R normalized above G for a red image");
    assert!(out[2 * plane] > out[plane], "B normalized above G for a black image");
}

/// The output dimension the downstream sqlite-vec table is sized for:
/// 512 floats (OpenCLIP ViT-B/32 image projection).
#[test]
fn embedding_dimension_is_512() {
    assert_eq!(CLIP_EMBEDDING_DIM, 512);
}
