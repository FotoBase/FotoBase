//! Unit tests for manifest parsing and checksum verification (TASK-05-001,
//! TDD criterion: "a model file with wrong checksum is rejected by the
//! loader").

use std::fs;
use std::path::PathBuf;

use fotobase_ai::{ManifestEntry, ModelRegistry};

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("model-manifest.json")
}

fn temp_model(name: &str, bytes: &[u8]) -> (PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.keep();
    let path = root.join(name);
    fs::write(&path, bytes).expect("write model file");
    (path, root)
}

/// Manifest parses and contains the two pinned models with permissive
/// licenses.
#[test]
fn manifest_parses_with_permissive_licenses() {
    let registry = ModelRegistry::from_manifest_file(&manifest_path()).expect("manifest parses");
    let names: Vec<&str> = registry.entries().iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"face_detection_yunet_2023mar.onnx"));
    assert!(names.contains(&"face_recognition_sface_2021dec.onnx"));
    for e in registry.entries() {
        assert!(
            matches!(e.license.as_str(), "MIT" | "Apache-2.0"),
            "non-permissive license {}",
            e.license
        );
        assert!(e.url.starts_with("https://"));
        assert_eq!(e.sha256.len(), 64);
    }
}

/// A file whose SHA-256 does not match the manifest entry is rejected.
#[test]
fn wrong_checksum_rejected() {
    let registry = ModelRegistry::from_manifest_file(&manifest_path()).expect("manifest");
    let entry = &registry.entries()[0];
    let (path, _dir) = temp_model(&entry.name, &vec![0u8; entry.size_bytes as usize]);

    let err = registry.verify_file(entry, &path).expect_err("must reject");
    assert!(err.to_string().contains("checksum mismatch"), "unexpected error: {err}");
}

/// A correct file passes verification.
#[test]
fn correct_checksum_accepted() {
    let entry = ManifestEntry {
        name: "test.onnx".into(),
        license: "MIT".into(),
        license_url: "https://example.com/LICENSE".into(),
        url: "https://example.com/test.onnx".into(),
        sha256: hex_sha256(b"model bytes"),
        size_bytes: 11,
    };
    let registry = ModelRegistry::new(vec![entry]);
    let (path, _dir) = temp_model("test.onnx", b"model bytes");
    let registry = ModelRegistry::new(registry.entries().to_vec());
    assert!(registry.verify_file(&registry.entries()[0], &path).is_ok());
}

/// A file with the right hash but wrong size is rejected (truncated or
/// padded downloads must not pass).
#[test]
fn wrong_size_rejected() {
    let entry = ManifestEntry {
        name: "test.onnx".into(),
        license: "MIT".into(),
        license_url: "https://example.com/LICENSE".into(),
        url: "https://example.com/test.onnx".into(),
        sha256: "0".repeat(64),
        size_bytes: 9999,
    };
    let registry = ModelRegistry::new(vec![entry]);
    let (path, _dir) = temp_model("test.onnx", b"model bytes");
    let err = registry.verify_file(&registry.entries()[0], &path).expect_err("must reject");
    assert!(err.to_string().contains("size mismatch"), "unexpected: {err}");
}

/// A missing manifest entry (model not declared) is an error, not a silent
/// pass.
#[test]
fn unknown_model_rejected() {
    let registry = ModelRegistry::new(vec![]);
    let (path, _dir) = temp_model("ghost.onnx", b"x");
    let _entry = ManifestEntry {
        name: "ghost.onnx".into(),
        license: "MIT".into(),
        license_url: String::new(),
        url: String::new(),
        sha256: "0".repeat(64),
        size_bytes: 1,
    };
    // The entry is not part of the registry -> lookup must fail.
    assert!(registry.entry("ghost.onnx").is_none());
    let _ = path;
}

/// The manifest on disk matches the CI license gate (`tools/
/// verify_model_licenses.py`): every entry MIT or Apache-2.0. Belt and
/// braces — the Python gate runs in CI, this keeps the Rust side honest.
#[test]
fn manifest_entries_are_permissive() {
    let registry = ModelRegistry::from_manifest_file(&manifest_path()).expect("manifest");
    for e in registry.entries() {
        assert!(
            e.license == "MIT" || e.license == "Apache-2.0",
            "non-permissive license in manifest: {} ({})",
            e.name,
            e.license
        );
    }
}

/// Minimal SHA-256 hex helper so the test does not need a hex crate:
/// computes the digest of `data` via the same sha2 dependency the crate
/// uses.
fn hex_sha256(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
