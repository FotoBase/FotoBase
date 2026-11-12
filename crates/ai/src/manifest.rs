//! Model manifest: the single source of truth for which models the app may
//! download, where from, and under which license (TASK-05-001, PLAN §4
//! pins, §10 model-licensing risk).
//!
//! The same manifest is checked in CI by `tools/verify_model_licenses.py`,
//! which fails the build if an entry is missing or non-permissive
//! (MIT/Apache-2.0, plus CC-BY-4.0 for attribution-only datasets such as
//! GeoNames — no InsightFace/non-commercial weights).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("manifest not found at {0}")]
    ManifestMissing(PathBuf),
    #[error("manifest invalid: {0}")]
    ManifestInvalid(String),
    #[error("checksum mismatch for {name}: expected {expected}, got {actual}")]
    ChecksumMismatch { name: String, expected: String, actual: String },
    #[error("size mismatch for {name}: expected {expected} bytes, got {actual}")]
    SizeMismatch { name: String, expected: u64, actual: u64 },
    #[error("unknown model: {0}")]
    UnknownModel(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// One downloadable model: name, pinned URL, expected SHA-256, size, and
/// the license the weights are distributed under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub name: String,
    pub license: String,
    #[serde(default)]
    pub license_url: String,
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
}

/// The set of models this build knows about. Loading is explicit: the app
/// prompts the user before any download (opt-in, PLAN §1).
#[derive(Debug, Clone)]
pub struct ModelRegistry {
    entries: Vec<ManifestEntry>,
}

impl ModelRegistry {
    pub fn new(entries: Vec<ManifestEntry>) -> Self {
        Self { entries }
    }

    pub fn from_manifest_file(path: &Path) -> Result<Self, ModelError> {
        let bytes = fs::read(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ModelError::ManifestMissing(path.to_path_buf()),
            _ => ModelError::Io(e),
        })?;
        Self::from_manifest_bytes(&bytes)
    }

    pub fn from_manifest_bytes(bytes: &[u8]) -> Result<Self, ModelError> {
        let entries: Vec<ManifestEntry> = serde_json::from_slice(bytes)
            .map_err(|e| ModelError::ManifestInvalid(e.to_string()))?;
        if entries.is_empty() {
            return Err(ModelError::ManifestInvalid("empty manifest".into()));
        }
        Ok(Self { entries })
    }

    pub fn entries(&self) -> &[ManifestEntry] {
        &self.entries
    }

    pub fn entry(&self, name: &str) -> Option<&ManifestEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Verify a downloaded (or installed) file against its manifest entry:
    /// exact size first (cheap), then SHA-256. A wrong checksum is rejected
    /// by the loader — the TDD criterion.
    pub fn verify_file(&self, entry: &ManifestEntry, path: &Path) -> Result<(), ModelError> {
        let data = fs::read(path)?;
        if data.len() as u64 != entry.size_bytes {
            return Err(ModelError::SizeMismatch {
                name: entry.name.clone(),
                expected: entry.size_bytes,
                actual: data.len() as u64,
            });
        }
        let actual = super::Sha256::digest_hex(&data);
        if !actual.eq_ignore_ascii_case(&entry.sha256) {
            return Err(ModelError::ChecksumMismatch {
                name: entry.name.clone(),
                expected: entry.sha256.clone(),
                actual,
            });
        }
        Ok(())
    }

    /// Whether the model is already installed (and intact) under
    /// `models_dir`. Callers use this to decide whether the opt-in prompt
    /// is needed at all.
    pub fn is_installed(&self, entry: &ManifestEntry, models_dir: &Path) -> bool {
        let path = models_dir.join(&entry.name);
        path.is_file() && self.verify_file(entry, &path).is_ok()
    }

    /// Install a verified model file into `models_dir`. The caller has
    /// already fetched the bytes after an explicit user prompt; this
    /// verifies the file *again* before moving it into place, atomically
    /// (`.tmp` + rename, same pattern as sidecar writes).
    pub fn install_model(
        &self,
        entry: &ManifestEntry,
        source: &Path,
        models_dir: &Path,
    ) -> Result<PathBuf, ModelError> {
        self.verify_file(entry, source)?;
        fs::create_dir_all(models_dir)?;
        let dest = models_dir.join(&entry.name);
        let tmp = models_dir.join(format!("{}.tmp", entry.name));
        fs::copy(source, &tmp)?;
        fs::rename(&tmp, &dest)?;
        Ok(dest)
    }

    /// Download one manifest entry over HTTPS into `models_dir` — the
    /// opt-in download flow (TASK-07-001, PLAN §1). The caller surfaces the
    /// consent prompt; nothing here downloads without being called. The
    /// bytes stream to a `.tmp` file, are verified against the pinned
    /// SHA-256 and size, then atomically renamed into place. A failed or
    /// tampered download leaves no installed file behind.
    #[cfg(feature = "download")]
    pub fn download_model(
        &self,
        entry: &ManifestEntry,
        models_dir: &Path,
    ) -> Result<PathBuf, ModelError> {
        fs::create_dir_all(models_dir)?;
        let tmp = models_dir.join(format!("{}.download.tmp", entry.name));
        let response = ureq::get(&entry.url).call().map_err(|e| {
            ModelError::ManifestInvalid(format!("download failed for {}: {e}", entry.name))
        })?;
        let mut reader = response.into_reader();
        let mut file = fs::File::create(&tmp)?;
        std::io::copy(&mut reader, &mut file)?;
        drop(file);
        match self.install_model(entry, &tmp, models_dir) {
            Ok(dest) => {
                let _ = fs::remove_file(&tmp);
                Ok(dest)
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                Err(e)
            }
        }
    }
}
