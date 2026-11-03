//! fotobase-ai: `ort` (ONNX Runtime) face detection (YuNet) + embedding
//! (MobileFaceNet-class SFace) with an opt-in, checksum-verified model
//! download flow (TASK-05-001, PLAN §4 pins).
//!
//! Invariants:
//! - CPU execution provider is the default; GPU providers are opt-in
//!   features (PLAN §4).
//! - Nothing downloads silently: callers must surface an explicit prompt
//!   before calling [`ModelRegistry::install_model`] (PLAN §1 "models are
//!   opt-in downloads").
//! - No telemetry: the crate performs no network calls except the explicit
//!   model download the user approved.

pub mod manifest;
pub mod runtime;

pub use manifest::{ManifestEntry, ModelError, ModelRegistry};
pub use runtime::{align_face, DetectedFace, FaceRuntime, RuntimeError};

/// Re-export the SHA-256 helper used for checksum verification.
pub use sha2;

/// Convenience: SHA-256 hex digest helper for tests and callers.
pub struct Sha256;

impl Sha256 {
    pub fn digest_hex(data: &[u8]) -> String {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(data);
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-ai";
