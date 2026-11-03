//! Perceptual hashing for the near-duplicate finder (TASK-04-019, PLAN §2
//! 2026 additions "Near-duplicate detection").
//!
//! The hash is derived data (PLAN §5 Authority): recomputable from the
//! original at any time, cached in the catalog's `files.phash`, never
//! written to sidecars. The same decode pipeline as the thumbnail path is
//! reused, so a RAW's embedded-JPEG fallback hashes the same pixels the
//! viewer sees.

use std::path::{Path, PathBuf};

use image_hasher::{HashAlg, HasherConfig};

use crate::{decode, RenderError};

/// A 64-bit perceptual hash (mean algorithm, 8×8), packed big-endian.
pub type PerceptualHash = [u8; 8];

/// Decode `src` and compute its perceptual hash.
pub fn perceptual_hash(src: &Path) -> Result<PerceptualHash, RenderError> {
    let bytes = std::fs::read(src)?;
    let (rgb, w, h) = decode(&bytes, src)?;
    let img = match image::RgbImage::from_raw(w, h, rgb) {
        Some(img) => img,
        None => {
            return Err(RenderError::Decode {
                path: src.to_path_buf(),
                detail: format!("decode returned a buffer that is not {w}×{h} RGB8"),
            });
        }
    };

    // Mean (aHash) at 8×8: cheap, deterministic, and robust across resize +
    // re-compress — the two transforms a "copy" duplicate goes through.
    let config = HasherConfig::new().hash_size(8, 8).hash_alg(HashAlg::Mean);
    let hasher = config.to_hasher();
    let hash = hasher.hash_image(&image::DynamicImage::ImageRgb8(img));
    let bytes = hash.as_bytes();
    let out = bytes.try_into().map_err(|_| {
        RenderError::UnsupportedFormat(format!("unexpected hash length {}", bytes.len()))
    })?;
    Ok(out)
}

/// [`perceptual_hash`] as a queue job (PLAN §3 rule 4): cooperative
/// cancellation before the decode; errors are results, not queue failures,
/// so one malformed photo never takes down the batch.
pub fn phash_job(src: PathBuf) -> fotobase_jobs::Job<Result<PerceptualHash, RenderError>> {
    fotobase_jobs::job(move |ctx| {
        let src = src.clone();
        async move {
            if ctx.is_cancelled() {
                return fotobase_jobs::Outcome::Cancelled;
            }
            fotobase_jobs::Outcome::Done(perceptual_hash(&src))
        }
    })
}
