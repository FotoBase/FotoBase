//! Integration tests: the download + install flow (TASK-05-001 TDD).
//!
//! The HTTP download itself is exercised with a local file:// -style stub:
//! `install_model` reads from a source path after checksum verification, so
//! the tests run fully offline (no network in unit tests).

use std::fs;
use std::path::{Path, PathBuf};

use fotobase_ai::{ModelRegistry, Sha256};

fn write_model(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p: PathBuf = dir.join(name);
    fs::write(&p, bytes).expect("write");
    p
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest_hex(data)
}

/// install_model copies a verified source file into the models directory
/// atomically (temp file + rename) and the installed file verifies.
#[test]
fn install_verified_model() {
    let bytes = b"fake but valid onnx payload";
    let src_dir = tempfile::tempdir().expect("tmp");
    let src = write_model(src_dir.path(), "model.onnx", bytes);

    let entry = fotobase_ai::ManifestEntry {
        name: "model.onnx".into(),
        license: "Apache-2.0".into(),
        license_url: "https://example.com".into(),
        url: "https://example.com/model.onnx".into(),
        sha256: sha256_hex(bytes),
        size_bytes: bytes.len() as u64,
    };
    let registry = ModelRegistry::new(vec![entry]);

    let models_dir = tempfile::tempdir().expect("tmp");
    registry.install_model(&registry.entries()[0], &src, models_dir.path()).expect("install");

    let installed = models_dir.path().join("model.onnx");
    assert!(installed.is_file());
    assert_eq!(fs::read(&installed).expect("read"), bytes);
    // And the installed copy verifies against the manifest.
    assert!(registry.verify_file(&registry.entries()[0], &installed).is_ok());
    // No temp leftovers.
    let leftovers: Vec<_> = fs::read_dir(models_dir.path())
        .expect("readdir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
}

/// A corrupt source file must not be installed (checksum gate before the
/// copy).
#[test]
fn install_rejects_bad_checksum() {
    let entry = fotobase_ai::ManifestEntry {
        name: "model.onnx".into(),
        license: "MIT".into(),
        license_url: String::new(),
        url: String::new(),
        sha256: "0".repeat(64),
        size_bytes: 4,
    };
    let registry = ModelRegistry::new(vec![entry]);
    let src_dir = tempfile::tempdir().expect("tmp");
    let src = write_model(src_dir.path(), "model.onnx", b"evil");
    let models_dir = tempfile::tempdir().expect("tmp");

    assert!(registry.install_model(&registry.entries()[0], &src, models_dir.path()).is_err());
    assert!(!models_dir.path().join("model.onnx").exists());
}

/// `is_installed` reflects the models dir state.
#[test]
fn is_installed_checks_dir() {
    let bytes = b"abc";
    let entry = fotobase_ai::ManifestEntry {
        name: "m.onnx".into(),
        license: "MIT".into(),
        license_url: String::new(),
        url: String::new(),
        sha256: sha256_hex(bytes),
        size_bytes: 3,
    };
    let registry = ModelRegistry::new(vec![entry]);
    let models_dir = tempfile::tempdir().expect("tmp");
    assert!(!registry.is_installed(&registry.entries()[0], models_dir.path()));
    fs::write(models_dir.path().join("m.onnx"), bytes).expect("write");
    assert!(registry.is_installed(&registry.entries()[0], models_dir.path()));
}
