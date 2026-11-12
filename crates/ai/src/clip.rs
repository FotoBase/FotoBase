//! CLIP ViT-B/32 image embeddings (TASK-07-001, PLAN §4 Semantic search
//! row, §6 P3 Smart search). OpenCLIP laion2b ViT-B/32 ONNX visual tower,
//! MIT-licensed, pinned in `model-manifest.json` — an opt-in download like
//! the face models (PLAN §1).
//!
//! Invariants:
//! - CPU execution provider only (same shared environment as the face
//!   runtime; GPU providers stay opt-in, PLAN §4).
//! - Input is a plain RGB8 buffer preprocessed exactly like OpenCLIP's
//!   reference transform (224×224, /255, CLIP mean/std) so embeddings are
//!   interoperable with the reference implementation.
//! - Output is the 512-d image projection, L2-normalized for cosine
//!   similarity (the sqlite-vec `photo_embeddings` table is sized for it).

use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::TensorRef,
};

/// The ViT-B/32 image tower's fixed input resolution.
pub const CLIP_INPUT: u32 = 224;
/// The image-projection dimension (the `photo_embeddings` vec0 width).
pub const CLIP_EMBEDDING_DIM: usize = 512;

/// OpenCLIP's CLIP-normalization constants (RGB channels).
const MEAN: [f32; 3] = [0.48145466, 0.4578275, 0.40821073];
// Clippy's excessive-precision would truncate these; the reference
// constants are kept verbatim for interoperability with OpenCLIP.
#[allow(clippy::excessive_precision)]
const STD: [f32; 3] = [0.26862954, 0.26130258, 0.27577711];

#[derive(Debug, thiserror::Error)]
pub enum ClipError {
    #[error("failed to load model {path}: {source}")]
    Load { path: std::path::PathBuf, source: ort::Error },
    #[error("inference failed: {0}")]
    Infer(#[from] ort::Error),
    #[error("unexpected model output shape: {0}")]
    Shape(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

/// Resize an RGB8 buffer to 224×224 and lay it out as NCHW f32 with
/// OpenCLIP's normalization (`/255`, then `(x - mean) / std` per channel).
/// Public so the preprocessing math is testable without the model.
pub fn clip_preprocess(rgb: &[u8], w: u32, h: u32) -> Vec<f32> {
    let n = CLIP_INPUT as usize;
    let plane = n * n;
    let mut out = vec![0f32; 3 * plane];
    let (wu, hu) = (w as usize, h as usize);
    for oy in 0..n {
        let sy = (((oy + 1) * hu / n).max(1)).min(hu) - 1;
        for ox in 0..n {
            let sx = (((ox + 1) * wu / n).max(1)).min(wu) - 1;
            let src = (sy * wu + sx) * 3;
            let dst = oy * n + ox;
            for c in 0..3 {
                out[c * plane + dst] = (rgb[src + c] as f32 / 255.0 - MEAN[c]) / STD[c];
            }
        }
    }
    out
}

/// A loaded CLIP visual tower on the shared CPU environment.
pub struct ClipRuntime {
    session: Session,
}

impl ClipRuntime {
    /// Load the visual tower with the CPU execution provider only.
    pub fn load(path: &std::path::Path) -> Result<Self, ClipError> {
        let mut builder = Session::builder(crate::runtime::environment())?;
        builder = builder
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| ClipError::Load { path: path.to_path_buf(), source: e.into() })?;
        let session = builder
            .commit_from_file(path)
            .map_err(|e| ClipError::Load { path: path.to_path_buf(), source: e })?;
        Ok(Self { session })
    }

    /// Embed one RGB8 image (contiguous, row-major, width*height*3).
    /// Returns the 512-d projection, L2-normalized. The session takes the
    /// first output tensor — the OpenCLIP export's image projection.
    pub fn embed_image(&mut self, rgb: &[u8], w: u32, h: u32) -> Result<Vec<f32>, ClipError> {
        let expected = w as usize * h as usize * 3;
        if rgb.len() != expected {
            return Err(ClipError::InvalidInput(format!(
                "expected {expected} bytes for {w}x{h} RGB, got {}",
                rgb.len()
            )));
        }
        let input = clip_preprocess(rgb, w, h);
        let tensor = TensorRef::from_array_view((
            [1usize, 3, CLIP_INPUT as usize, CLIP_INPUT as usize],
            &*input,
        ))?;
        let outputs = self.session.run(ort::inputs![tensor])?;
        let out = outputs[0].view().downcast::<ort::value::TensorValueType<f32>>()?;
        let (shape, data) = out.extract_tensor();
        let dims: Vec<usize> = shape.iter().map(|&d| d as usize).collect();
        let d = match dims.last() {
            Some(&d) if d == CLIP_EMBEDDING_DIM => d,
            _ => return Err(ClipError::Shape(format!("unexpected CLIP output {dims:?}"))),
        };
        let mut v: Vec<f32> = data[data.len() - d..].to_vec();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-9 {
            for x in &mut v {
                *x /= norm;
            }
        }
        Ok(v)
    }
}

/// The OpenAI CLIP BPE tokenizer (OpenCLIP uses the same): SOT/EOT markers,
/// the 77-token context length, and a padded id vector for the text tower.
/// A thin, testable façade over `instant-clip-tokenizer` (TASK-07-002).
pub mod clip_tokenize {
    use instant_clip_tokenizer::{Token, Tokenizer};

    /// The ViT-B/32 text tower's context length.
    pub const CTX: usize = 77;

    fn tokenizer() -> &'static Tokenizer {
        use std::sync::OnceLock;
        static TOK: OnceLock<Tokenizer> = OnceLock::new();
        TOK.get_or_init(Tokenizer::new)
    }

    /// The `<start_of_text>` marker id.
    pub const SOT: i64 = 49406;
    /// The `<end_of_text>` marker id.
    pub const EOT: i64 = 49407;

    /// Append the BPE ids of `text` (lowercased like the reference
    /// implementation) to `out`.
    pub fn encode(text: &str, out: &mut Vec<i64>) {
        let mut raw: Vec<Token> = Vec::new();
        tokenizer().encode(text, &mut raw);
        out.extend(raw.iter().map(|t| t.to_u16() as i64));
    }

    /// One 77-token int64 row for the text tower: SOT, the prompt ids, EOT,
    /// then zero padding (the OpenAI CLIP convention). A prompt longer than
    /// the context is truncated at the EOT slot.
    pub fn tokenize_padded(text: &str) -> Vec<i64> {
        let mut ids = vec![SOT];
        encode(text, &mut ids);
        ids.truncate(CTX - 1);
        ids.push(EOT);
        ids.resize(CTX, 0);
        ids
    }
}

/// A loaded CLIP textual tower on the shared CPU environment (TASK-07-002):
/// int64 token ids `[batch, 77]` in, the 512-d text projection out — the
/// same space as the image embeddings, so cosine distance ranks photos.
pub struct ClipTextRuntime {
    session: Session,
}

impl ClipTextRuntime {
    /// Load the textual tower with the CPU execution provider only.
    pub fn load(path: &std::path::Path) -> Result<Self, ClipError> {
        let mut builder = Session::builder(crate::runtime::environment())?;
        builder = builder
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| ClipError::Load { path: path.to_path_buf(), source: e.into() })?;
        let session = builder
            .commit_from_file(path)
            .map_err(|e| ClipError::Load { path: path.to_path_buf(), source: e })?;
        Ok(Self { session })
    }

    /// Embed one text query: tokenize to the 77-token context, run the
    /// textual tower, return the L2-normalized 512-d projection.
    pub fn embed_text(&mut self, text: &str) -> Result<Vec<f32>, ClipError> {
        if text.trim().is_empty() {
            return Err(ClipError::InvalidInput("empty text query".into()));
        }
        let ids = clip_tokenize::tokenize_padded(text);
        let tensor = TensorRef::from_array_view(([1usize, 77usize], ids.as_slice()))?;
        let outputs = self.session.run(ort::inputs![tensor])?;
        let out = outputs[0].view().downcast::<ort::value::TensorValueType<f32>>()?;
        let (shape, data) = out.extract_tensor();
        let dims: Vec<usize> = shape.iter().map(|&d| d as usize).collect();
        let d = match dims.last() {
            Some(&d) if d == CLIP_EMBEDDING_DIM => d,
            _ => return Err(ClipError::Shape(format!("unexpected CLIP text output {dims:?}"))),
        };
        let mut v: Vec<f32> = data[data.len() - d..].to_vec();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-9 {
            for x in &mut v {
                *x /= norm;
            }
        }
        Ok(v)
    }
}
