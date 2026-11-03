//! Integration tests for the YuNet face detection + SFace embedding
//! runtime path (TASK-05-001 TDD criterion: "YuNet fixture image →
//! expected number/locations of detected faces").
//!
//! These tests are **gated on real models**: they run only when the models
//! are present in `target/ai-models/` (populated by
//! `tools/download_models.sh`). CI runs the manifest/checksum gate always;
//! the model-execution leg runs wherever the weights are available,
//! keeping `cargo test --workspace` green and fast on machines without
//! the ~39 MB weights.

use std::path::PathBuf;

use fotobase_ai::DetectedFace;

fn models_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ai-models");
    let yunet = dir.join("face_detection_yunet_2023mar.onnx");
    let sface = dir.join("face_recognition_sface_2021dec.onnx");
    if yunet.is_file() && sface.is_file() {
        Some(dir)
    } else {
        None
    }
}

fn load() -> Option<fotobase_ai::FaceRuntime> {
    let dir = models_dir()?;
    Some(
        fotobase_ai::FaceRuntime::load(
            &dir.join("face_detection_yunet_2023mar.onnx"),
            &dir.join("face_recognition_sface_2021dec.onnx"),
        )
        .expect("runtime loads on CPU"),
    )
}

/// A plain synthetic gradient (no faces) must produce zero detections —
/// no false positives on plain scenes — and the runtime must construct
/// without a GPU.
#[test]
fn detector_constructs_cpu_only_and_reports_no_faces_on_plain_image() {
    let Some(mut runtime) = load() else {
        eprintln!("skipping: models not downloaded into target/ai-models");
        return;
    };
    let mut rgb = vec![0u8; 640 * 640 * 3];
    for (i, px) in rgb.chunks_mut(3).enumerate() {
        let v = (i % 640) as u8;
        px[0] = v;
        px[1] = v;
        px[2] = v;
    }
    let faces = runtime.detect_faces(&rgb, 640, 640).expect("detect");
    assert!(faces.is_empty(), "false positives on plain image: {faces:?}");
}

/// The detector must accept odd-sized inputs (the resample path must not
/// assume dimensions divisible by the detector grid).
#[test]
fn detector_handles_non_grid_input_sizes() {
    let Some(mut runtime) = load() else {
        eprintln!("skipping: models not downloaded into target/ai-models");
        return;
    };
    let rgb = vec![128u8; 333 * 217 * 3];
    let faces = runtime.detect_faces(&rgb, 333, 217).expect("detect");
    assert!(faces.is_empty());
}

/// Embedding sanity: deterministic, 128-dimensional, unit-normalized
/// (SFace output convention, matching OpenCV's pre-`match` normalize).
#[test]
fn embedding_is_deterministic_and_normalized() {
    let Some(mut runtime) = load() else {
        eprintln!("skipping: models not downloaded into target/ai-models");
        return;
    };
    let rgb = vec![128u8; 112 * 112 * 3];
    let a = runtime.embed_face(&rgb, 112, 112).expect("embed");
    let b = runtime.embed_face(&rgb, 112, 112).expect("embed");
    assert_eq!(a.len(), 128, "SFace embedding dimensionality");
    assert_eq!(a, b, "embedding must be deterministic");

    let norm: f32 = a.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-3, "embedding not unit-normalized: {norm}");
}

/// Wrong-size inputs must be rejected with a clear error, not panicked on.
#[test]
fn wrong_buffer_size_rejected() {
    let Some(mut runtime) = load() else {
        eprintln!("skipping: models not downloaded into target/ai-models");
        return;
    };
    let rgb = vec![0u8; 10];
    assert!(runtime.detect_faces(&rgb, 64, 64).is_err());
    assert!(runtime.embed_face(&rgb, 64, 64).is_err());
}

/// The alignment warp must map the reference landmarks to themselves: an
/// input whose landmarks are already the ArcFace reference points warps
/// to (approximately) itself.
#[test]
fn align_face_is_identity_for_reference_landmarks() {
    let reference: [f32; 10] =
        [38.2946, 51.6963, 73.5318, 51.5014, 56.0252, 71.7366, 45.0252, 92.3655, 73.5318, 92.2041];
    let rgb = vec![200u8; 112 * 112 * 3];
    let aligned = fotobase_ai::align_face(&rgb, 112, 112, &reference).expect("align");
    // Every interior pixel (the warp covers the whole 112×112 target for
    // an identity mapping) must be the source color.
    assert!(aligned.iter().all(|&b| b == 200));
}

/// The face-count leg runs only when a face image is provided via
/// `FOTOBASE_FACE_FIXTURE` (a photo is not committed to the repo).
#[test]
fn fixture_face_is_detected() {
    let Some(path) = std::env::var_os("FOTOBASE_FACE_FIXTURE") else {
        eprintln!("skipping: FOTOBASE_FACE_FIXTURE not set");
        return;
    };
    let Some(mut runtime) = load() else {
        eprintln!("skipping: models not downloaded into target/ai-models");
        return;
    };
    let img = image::open(&path).expect("fixture image").to_rgb8();
    let (w, h) = (img.width(), img.height());
    let faces = runtime.detect_faces(img.as_raw(), w, h).expect("detect");
    assert!(!faces.is_empty(), "no face detected in fixture {path:?}");
    let best: &DetectedFace =
        faces.iter().max_by(|a, b| a.confidence.total_cmp(&b.confidence)).expect("non-empty");
    assert!(best.confidence > 0.8, "weak detection: {best:?}");
    // Landmarks must fall inside the image bounds.
    for lm in best.landmarks.chunks(2) {
        assert!(lm[0] >= 0.0 && lm[0] < w as f32, "landmark x out of bounds");
        assert!(lm[1] >= 0.0 && lm[1] < h as f32, "landmark y out of bounds");
    }
}
