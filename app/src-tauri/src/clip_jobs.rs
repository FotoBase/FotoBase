//! CLIP image embedding passes as job-queue jobs (TASK-07-001, PLAN §6 P3
//! Smart search, §3 rule 4, §5 derived data).
//!
//! A full-library pass walks every cataloged image, computes the OpenCLIP
//! ViT-B/32 image embedding and stores it in the catalog's
//! `photo_embeddings` sqlite-vec table. `files.embed_scanned` marks 0
//! pending / 1 done / 2 failed (the phash sentinel pattern), so a pass is
//! incremental and a cancelled pass leaves valid state. Everything written
//! is derived data in the catalog (PLAN §5) — embeddings are never written
//! to sidecars, and a catalog rebuild re-derives them.
//!
//! [`ClipEngine`] abstracts the ONNX runtime so every scheduling and
//! catalog rule is testable without the 350 MB model; [`OrtEngine`] is the
//! production wrapper over `fotobase_ai::ClipRuntime`. Images decode to
//! in-memory RGB through `fotobase_render::decode_rgb` — no image bytes
//! ever travel over IPC (PLAN §3 rule 3).
//!
//! Battery: the queue this module runs on shares the face queue's
//! [`fotobase_jobs::BatteryHook`] — on battery the dequeue pauses (§10).
//! Background priority only: visible thumbnails are never starved.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use fotobase_ai::{ClipRuntime, ClipTextRuntime};
use fotobase_catalog::Catalog;
use serde::Serialize;

/// Tauri event the embedding pass reports progress on (§3: plain counters,
/// no image bytes).
pub const EMBEDDINGS_EVENT: &str = "embeddings-progress";

/// The manifest entry name of the CLIP visual tower (TASK-07-001 pin).
pub const CLIP_MODEL_NAME: &str = "open_clip_vit_b32_laion2b_visual.onnx";

/// The manifest entry name of the CLIP textual tower (TASK-07-002 pin).
pub const CLIP_TEXT_MODEL_NAME: &str = "open_clip_vit_b32_laion2b_textual.onnx";

/// One full pass over the unprocessed images.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EmbedReport {
    /// Photos embedded on this pass.
    pub embedded: usize,
    /// Photos that could not be decoded or embedded (marked failed; a later
    /// explicit reset retries them).
    pub failed: usize,
}

/// The embedding backend.
pub trait ClipEngine: Send {
    fn embed(&mut self, rgb: &[u8], width: u32, height: u32) -> Result<Vec<f32>, String>;
}

/// Production engine: OpenCLIP ViT-B/32 on the CPU ort runtime (the
/// manifest-pinned, MIT-licensed opt-in download).
pub struct OrtEngine {
    runtime: ClipRuntime,
}

impl OrtEngine {
    /// Load the visual tower from the catalog location's model directory
    /// (the registry from 05-001 owns checksums and installation).
    pub fn load(model_dir: &std::path::Path) -> Result<Self, String> {
        let runtime =
            ClipRuntime::load(&model_dir.join(CLIP_MODEL_NAME)).map_err(|e| e.to_string())?;
        Ok(Self { runtime })
    }
}

impl ClipEngine for OrtEngine {
    fn embed(&mut self, rgb: &[u8], width: u32, height: u32) -> Result<Vec<f32>, String> {
        self.runtime.embed_image(rgb, width, height).map_err(|e| e.to_string())
    }
}

/// The text-side backend (TASK-07-002).
pub trait TextEngine: Send {
    fn embed_text(&mut self, text: &str) -> Result<Vec<f32>, String>;
}

/// Production text engine: the OpenCLIP ViT-B/32 textual tower (the
/// manifest-pinned, MIT-licensed opt-in download).
pub struct OrtTextEngine {
    runtime: ClipTextRuntime,
}

impl OrtTextEngine {
    /// Load the textual tower from the models directory.
    pub fn load(model_dir: &std::path::Path) -> Result<Self, String> {
        let runtime = ClipTextRuntime::load(&model_dir.join(CLIP_TEXT_MODEL_NAME))
            .map_err(|e| e.to_string())?;
        Ok(Self { runtime })
    }
}

impl TextEngine for OrtTextEngine {
    fn embed_text(&mut self, text: &str) -> Result<Vec<f32>, String> {
        self.runtime.embed_text(text).map_err(|e| e.to_string())
    }
}

/// The text→image query path (TASK-07-002, PLAN §4 Vector search): encode
/// the query with the CLIP text encoder, then rank the stored photo
/// embeddings by cosine distance. Returns `(file_id, distance)` pairs,
/// closest first. No image bytes here — the caller resolves ids to rows
/// (§3 rule 3).
pub fn search_photos(
    cat: &mut Catalog,
    engine: &mut dyn TextEngine,
    text: &str,
    k: usize,
) -> Result<Vec<(i64, f32)>, String> {
    if text.trim().is_empty() {
        return Err("empty text query".into());
    }
    let query = engine.embed_text(text)?;
    cat.search_photo_embeddings(&query, k).map_err(|e| e.to_string())
}

/// One full-library embedding pass. Cooperative cancellation: a cancelled
/// pass leaves valid state — unprocessed rows stay pending, a later pass
/// picks them up.
pub fn run_embed_pass(
    cat: &mut Catalog,
    engine: &mut dyn ClipEngine,
    cancel: &AtomicBool,
) -> Result<EmbedReport, String> {
    let mut report = EmbedReport::default();
    for (file_id, path) in cat.embed_candidates().map_err(|e| e.to_string())? {
        if cancel.load(Ordering::Relaxed) {
            return Ok(report);
        }
        let rgb = match fotobase_render::decode_rgb(std::path::Path::new(&path)) {
            Ok((rgb, w, h)) => (rgb, w, h),
            Err(_) => {
                // Park the row (sentinel) so one undecodable photo neither
                // blocks the pass forever nor inflates later counts.
                cat.mark_embed_scanned(file_id, false).map_err(|e| e.to_string())?;
                report.failed += 1;
                continue;
            }
        };
        match engine.embed(&rgb.0, rgb.1, rgb.2) {
            Ok(vector) => {
                cat.store_photo_embedding(file_id, &vector).map_err(|e| e.to_string())?;
                cat.mark_embed_scanned(file_id, true).map_err(|e| e.to_string())?;
                report.embedded += 1;
            }
            Err(_) => {
                cat.mark_embed_scanned(file_id, false).map_err(|e| e.to_string())?;
                report.failed += 1;
            }
        }
    }
    Ok(report)
}

/// Monotonic sequence for embedding-pass job keys (one full pass at a time
/// coalesces; a re-trigger while a pass runs attaches to it).
static PASS_SEQ: AtomicU64 = AtomicU64::new(0);

/// Job key for the next full-library pass.
pub fn job_key() -> String {
    format!("embed-pass-{}", PASS_SEQ.fetch_add(1, Ordering::Relaxed))
}

/// The last semantic query's encoded text vector (TASK-07-003): a text-tower
/// inference costs far more than a string compare, so the one-slot cache
/// lets a re-typed (or unchanged, debounced) query skip the engine
/// entirely. In-memory only — the vector is derived data, never persisted.
#[derive(Default)]
pub struct TextVectorCache(std::sync::Mutex<Option<(String, Vec<f32>)>>);

impl TextVectorCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The cached vector for `text`, or `encode(text)`'s result (which then
    /// becomes the cached slot). An encode failure leaves the cache alone.
    pub fn get_or_encode(
        &self,
        text: &str,
        encode: impl Fn(&str) -> Result<Vec<f32>, String>,
    ) -> Result<Vec<f32>, String> {
        if let Ok(guard) = self.0.lock() {
            if let Some((cached, vector)) = guard.as_ref() {
                if cached == text {
                    return Ok(vector.clone());
                }
            }
        }
        let vector = encode(text)?;
        if let Ok(mut guard) = self.0.lock() {
            *guard = Some((text.to_string(), vector.clone()));
        }
        Ok(vector)
    }

    /// The cached vector for `text`, if one is stored.
    pub fn get(&self, text: &str) -> Option<Vec<f32>> {
        let guard = self.0.lock().ok()?;
        let (cached, vector) = guard.as_ref()?;
        (cached == text).then(|| vector.clone())
    }

    /// Store `text`'s vector as the (single-slot) cache content.
    pub fn put(&self, text: &str, vector: Vec<f32>) {
        if let Ok(mut guard) = self.0.lock() {
            *guard = Some((text.to_string(), vector));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_identity() {
        // The lib.rs smoke test list expects every module to be reachable.
        assert!(!EMBEDDINGS_EVENT.is_empty());
    }

    /// TASK-07-003: the last query's encoded text vector is cached — a
    /// second call with the same text must not re-encode.
    #[test]
    fn text_vector_cache_skips_reencoding() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let cache = TextVectorCache::new();
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let encode = |_: &str| -> Result<Vec<f32>, String> {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(vec![0.5; 512])
        };
        let first = cache.get_or_encode("dog on the beach", encode).unwrap();
        let second = cache.get_or_encode("dog on the beach", encode).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 1, "the second call hit the cache");
        assert_eq!(first, second);
        // A different text re-encodes and replaces the slot.
        let third = cache.get_or_encode("sunset", encode).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert_eq!(third, vec![0.5; 512]);
    }

    #[test]
    fn text_vector_cache_survives_an_encode_failure() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let cache = TextVectorCache::new();
        assert!(cache.get_or_encode("", |_| Err("boom".into())).is_err(), "the failure surfaces");
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let encode = |_: &str| -> Result<Vec<f32>, String> {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(vec![1.0])
        };
        let ok = cache.get_or_encode("a", encode).unwrap();
        // The failed attempt left nothing cached, so "a" encoded fresh once;
        // a repeat now hits the cache.
        let again = cache.get_or_encode("a", encode).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 1, "the empty-text failure cached nothing");
        assert_eq!(ok, again);
    }
}
