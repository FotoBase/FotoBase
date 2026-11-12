//! TASK-07-001 TDD: the opt-in model download. The HTTP fetch runs against
//! a local HTTP stub on 127.0.0.1 — fully offline, no network in unit
//! tests. The download must verify SHA-256 + size before the model lands
//! in the models directory, atomically, and leave nothing behind on a bad
//! checksum.
#![cfg(feature = "download")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use fotobase_ai::{ManifestEntry, ModelRegistry, Sha256};

/// Serve `payload` once (or until the caller closes) on an ephemeral
/// 127.0.0.1 port; returns the `http://127.0.0.1:<port>/model.onnx` URL.
fn serve_once(payload: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&payload);
        }
    });
    format!("http://127.0.0.1:{port}/model.onnx")
}

fn entry_for(payload: &[u8], url: String) -> ManifestEntry {
    ManifestEntry {
        name: "model.onnx".into(),
        license: "MIT".into(),
        license_url: String::new(),
        url,
        sha256: Sha256::digest_hex(payload),
        size_bytes: payload.len() as u64,
    }
}

#[test]
fn download_verifies_and_installs_atomically() {
    let payload = b"fake but valid onnx payload".to_vec();
    let url = serve_once(payload.clone());
    let entry = entry_for(&payload, url);
    let registry = ModelRegistry::new(vec![entry.clone()]);

    let models_dir = tempfile::tempdir().expect("tmp");
    let installed = registry.download_model(&entry, models_dir.path()).expect("download");
    assert_eq!(installed, models_dir.path().join("model.onnx"));
    assert_eq!(std::fs::read(&installed).expect("read"), payload);
    // No download leftovers.
    let leftovers: Vec<_> = std::fs::read_dir(models_dir.path())
        .expect("readdir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
}

/// A tampered download must not be installed: checksum gate before the
/// rename, no model file, no leftover temp file.
#[test]
fn download_rejects_tampered_payload() {
    let url = serve_once(b"tampered bytes".to_vec());
    let entry = entry_for(b"the real payload", url);
    let registry = ModelRegistry::new(vec![entry.clone()]);

    let models_dir = tempfile::tempdir().expect("tmp");
    let err = registry
        .download_model(&entry, models_dir.path())
        .expect_err("tampered payload must be rejected");
    assert!(
        err.to_string().contains("checksum mismatch") || err.to_string().contains("size mismatch"),
        "unexpected error: {err}"
    );
    assert!(!models_dir.path().join("model.onnx").exists(), "nothing installed");
}
