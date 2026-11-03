//! The ONNX runtime wrapper: YuNet face detection + SFace embedding, CPU
//! execution provider by default (TASK-05-001, PLAN §4).
//!
//! Input/output are plain in-memory RGB buffers — no image bytes travel
//! over IPC and the crate never touches originals (read-only invariant).
//! Telemetry is explicitly disabled at the environment level (no-telemetry
//! invariant, PLAN §1).
//!
//! Preprocessing matches OpenCV's `FaceDetectorYN` / `FaceRecognizerSF`
//! exactly (uint8 RGB, scale 1.0, no mean subtraction), so embeddings are
//! interoperable with the reference implementation.

use std::path::Path;
use std::sync::OnceLock;

use ort::{
    environment::Environment,
    session::{builder::GraphOptimizationLevel, Session, SessionOutputs},
    value::{Tensor, TensorRef, TensorValueType},
};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("failed to load model {path}: {source}")]
    Load { path: std::path::PathBuf, source: ort::Error },
    #[error("inference failed: {0}")]
    Infer(#[from] ort::Error),
    #[error("unexpected model output shape: {0}")]
    Shape(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

/// One detected face: axis-aligned box in pixel coordinates, the five
/// landmarks (right eye, left eye, nose tip, right mouth corner, left
/// mouth corner), and the detector confidence.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedFace {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// 10 floats: (x, y) per landmark, in input-image coordinates.
    pub landmarks: [f32; 10],
    pub confidence: f32,
}

static ENV: OnceLock<Environment> = OnceLock::new();

/// The shared ONNX Runtime environment: no GPU execution providers
/// registered (CPU fallback) and telemetry disabled — the no-telemetry
/// invariant is enforced here, not left to callers.
fn environment() -> &'static Environment {
    ENV.get_or_init(|| {
        ort::environment::init().with_telemetry(false).build().expect("onnxruntime environment")
    })
}

/// The five-point alignment target used by OpenCV's `alignCrop` (ArcFace
/// canonical 112×112 coordinates). Landmark order: right eye, left eye,
/// nose tip, right mouth corner, left mouth corner.
const ARC_REFERENCE: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [45.0252, 92.3655],
    [73.5318, 92.2041],
];

/// A loaded detection+embedding pair of sessions on the shared CPU
/// environment.
pub struct FaceRuntime {
    detector: Session,
    embedder: Session,
}

impl FaceRuntime {
    /// YuNet 2023mar ships a fixed 640×640 input.
    const DETECTOR_INPUT: u32 = 640;
    /// SFace-class embedders use 112×112 aligned crops.
    const EMBEDDER_INPUT: u32 = 112;

    /// Load both models with the CPU execution provider only — no GPU
    /// requirement (functional spec: "Face detection + embedding run on
    /// CPU with no GPU requirement").
    pub fn load(detector_path: &Path, embedder_path: &Path) -> Result<Self, RuntimeError> {
        let detector = Self::build_session(detector_path)?;
        let embedder = Self::build_session(embedder_path)?;
        Ok(Self { detector, embedder })
    }

    fn build_session(path: &Path) -> Result<Session, RuntimeError> {
        let mut builder = Session::builder(environment())?;
        builder = builder
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| RuntimeError::Load { path: path.to_path_buf(), source: e.into() })?;
        builder
            .commit_from_file(path)
            .map_err(|e| RuntimeError::Load { path: path.to_path_buf(), source: e })
    }

    /// Detect faces in an RGB8 buffer (contiguous, row-major,
    /// width*height*3). Coordinates are returned in the input resolution.
    ///
    /// The buffer is resized to the detector's fixed input (640×640);
    /// decoded boxes/landmarks are scaled back to the caller's resolution.
    pub fn detect_faces(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<DetectedFace>, RuntimeError> {
        Self::check_rgb(rgb, width, height)?;
        let n = Self::DETECTOR_INPUT;
        let input = resize_nchw_f32(rgb, width, height, n, n);
        // OpenCV blobFromImage defaults: scale 1.0, mean 0, swapRB — the
        // model consumes raw RGB values in [0, 255].
        let tensor = TensorRef::from_array_view(([1usize, 3, n as usize, n as usize], &*input))?;
        let outputs = self.detector.run(ort::inputs![tensor])?;

        // Per-stride decode, replicating OpenCV's FaceDetectorYN post
        // process: score = sqrt(clamp(cls) * clamp(obj)); box center =
        // (grid + bbox.xy) * stride; size = exp(bbox.wh) * stride;
        // landmarks = (kps + grid) * stride.
        let mut faces = Vec::new();
        for (oi, stride) in [8u32, 16, 32].into_iter().enumerate() {
            let cls = extract(&outputs, oi)?;
            let obj = extract(&outputs, oi + 3)?;
            let bbox = extract(&outputs, oi + 6)?;
            let kps = extract(&outputs, oi + 9)?;
            let grid = (n / stride) as usize;
            let count = cls.len();
            let sx = width as f32 / n as f32;
            let sy = height as f32 / n as f32;
            for idx in 0..count {
                let score = (cls[idx].clamp(0.0, 1.0) * obj[idx].clamp(0.0, 1.0)).sqrt();
                if score < 0.6 {
                    continue;
                }
                let r = idx / grid;
                let c = idx % grid;
                let cx = (c as f32 + bbox[idx * 4]) * stride as f32;
                let cy = (r as f32 + bbox[idx * 4 + 1]) * stride as f32;
                let w = bbox[idx * 4 + 2].exp() * stride as f32;
                let h = bbox[idx * 4 + 3].exp() * stride as f32;
                let mut landmarks = [0f32; 10];
                for k in 0..5 {
                    landmarks[k * 2] = (kps[idx * 10 + k * 2] + c as f32) * stride as f32 * sx;
                    landmarks[k * 2 + 1] =
                        (kps[idx * 10 + k * 2 + 1] + r as f32) * stride as f32 * sy;
                }
                faces.push(DetectedFace {
                    x: (cx - w / 2.0) * sx,
                    y: (cy - h / 2.0) * sy,
                    width: w * sx,
                    height: h * sy,
                    landmarks,
                    confidence: score,
                });
            }
        }
        Ok(faces)
    }

    /// Embed an aligned face crop: RGB8 (contiguous, width*height*3). The
    /// crop is resized to 112×112 and fed to the embedder with OpenCV's
    /// SFace preprocessing (scale 1.0, RGB, [0, 255]). Returns the raw
    /// 128-d vector, unit-normalized for cosine matching (matching the
    /// `normalize()` OpenCV applies before `match`).
    pub fn embed_face(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<f32>, RuntimeError> {
        Self::check_rgb(rgb, width, height)?;
        let n = Self::EMBEDDER_INPUT;
        let input = resize_nchw_f32(rgb, width, height, n, n);
        let tensor = TensorRef::from_array_view(([1usize, 3, n as usize, n as usize], &*input))?;
        let outputs = self.embedder.run(ort::inputs![tensor])?;
        let out = outputs[0].view().downcast::<TensorValueType<f32>>()?;
        let (shape, data) = out.extract_tensor();
        let dims: Vec<usize> = shape.iter().map(|&d| d as usize).collect();
        // Output may be [1, D] or [D].
        let d = match dims.len() {
            1 => dims[0],
            2 => dims[1],
            _ => return Err(RuntimeError::Shape(format!("unexpected embed output {dims:?}"))),
        };
        let mut v: Vec<f32> = data[..d].to_vec();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-9 {
            for x in &mut v {
                *x /= norm;
            }
        }
        Ok(v)
    }

    fn check_rgb(rgb: &[u8], width: u32, height: u32) -> Result<(), RuntimeError> {
        let expected = width as usize * height as usize * 3;
        if rgb.len() != expected {
            return Err(RuntimeError::InvalidInput(format!(
                "expected {expected} bytes for {width}x{height} RGB, got {}",
                rgb.len()
            )));
        }
        Ok(())
    }
}

/// Extract output `i` as a flat f32 vec.
fn extract(outputs: &SessionOutputs<'_>, i: usize) -> Result<Vec<f32>, RuntimeError> {
    let out = outputs[i].view().downcast::<TensorValueType<f32>>()?;
    let (_, data) = out.extract_tensor();
    Ok(data.to_vec())
}

/// Resize an RGB8 buffer to (out_w, out_h) and lay out as NCHW, keeping u8
/// values (the models want raw [0, 255] RGB — see module docs).
fn resize_nchw_f32(rgb: &[u8], w: u32, h: u32, out_w: u32, out_h: u32) -> Vec<f32> {
    let mut out = vec![0f32; (3 * out_w * out_h) as usize];
    let plane = (out_w * out_h) as usize;
    let (wu, hu) = (w as usize, h as usize);
    for oy in 0..out_h {
        let sy = (((oy as u64 + 1) * h as u64 / out_h as u64) as usize).min(hu - 1);
        for ox in 0..out_w {
            let sx = (((ox as u64 + 1) * w as u64 / out_w as u64) as usize).min(wu - 1);
            let src = (sy * wu + sx) * 3;
            let dst = (oy as usize) * out_w as usize + ox as usize;
            out[dst] = rgb[src] as f32;
            out[plane + dst] = rgb[src + 1] as f32;
            out[2 * plane + dst] = rgb[src + 2] as f32;
        }
    }
    out
}

/// Landmark-based similarity alignment (the `alignCrop` equivalent): a
/// similarity transform mapping the five detected landmarks to the ArcFace
/// reference points, then a nearest-neighbor warp into a 112×112 RGB
/// buffer ready for [`FaceRuntime::embed_face`].
pub fn align_face(
    rgb: &[u8],
    width: u32,
    height: u32,
    landmarks: &[f32; 10],
) -> Result<Vec<u8>, RuntimeError> {
    FaceRuntime::check_rgb(rgb, width, height)?;
    let src = [
        [landmarks[0], landmarks[1]],
        [landmarks[2], landmarks[3]],
        [landmarks[4], landmarks[5]],
        [landmarks[6], landmarks[7]],
        [landmarks[8], landmarks[9]],
    ];
    let t = similarity_transform(&src, &ARC_REFERENCE);
    let inv =
        invert2x3(&t).ok_or_else(|| RuntimeError::InvalidInput("degenerate alignment".into()))?;
    let out_w = FaceRuntime::EMBEDDER_INPUT as usize;
    let mut out = vec![0u8; 3 * out_w * out_w];
    let (wu, hu) = (width as usize, height as usize);
    for dy in 0..out_w {
        for dx in 0..out_w {
            let (sx, sy) = apply2x3(&inv, dx as f32, dy as f32);
            let sxi = sx.round() as i64;
            let syi = sy.round() as i64;
            if sxi >= 0 && syi >= 0 && (sxi as usize) < wu && (syi as usize) < hu {
                let s = ((syi as usize) * wu + (sxi as usize)) * 3;
                let dst = (dy * out_w + dx) * 3;
                out[dst] = rgb[s];
                out[dst + 1] = rgb[s + 1];
                out[dst + 2] = rgb[s + 2];
            }
        }
    }
    Ok(out)
}

/// Least-squares similarity transform (rotation + uniform scale +
/// translation) mapping `src` -> `dst` over the five points, as a 2×3
/// matrix (Umeyama without reflection, closed form on complex numbers).
fn similarity_transform(src: &[[f32; 2]; 5], dst: &[[f32; 2]; 5]) -> [f32; 6] {
    let n = src.len() as f32;
    let ms = (src.iter().map(|p| p[0]).sum::<f32>() / n, src.iter().map(|p| p[1]).sum::<f32>() / n);
    let md = (dst.iter().map(|p| p[0]).sum::<f32>() / n, dst.iter().map(|p| p[1]).sum::<f32>() / n);
    let (mut a, mut b) = (0f32, 0f32);
    let mut norm_src = 0f32;
    for (s, d) in src.iter().zip(dst.iter()) {
        let sx = s[0] - ms.0;
        let sy = s[1] - ms.1;
        let dx = d[0] - md.0;
        let dy = d[1] - md.1;
        a += sx * dx + sy * dy;
        b += sx * dy - sy * dx;
        norm_src += sx * sx + sy * sy;
    }
    if norm_src < 1e-12 {
        return [1.0, 0.0, md.0, 0.0, 1.0, md.1];
    }
    // scale = |Σ conj(src)·dst| / Σ|src|²; angle = arg(Σ conj(src)·dst)
    let scale = (a * a + b * b).sqrt() / norm_src;
    let theta = b.atan2(a);
    let (c, sn) = (theta.cos(), theta.sin());
    let m00 = scale * c;
    let m01 = -scale * sn;
    let tx = md.0 - (m00 * ms.0 + m01 * ms.1);
    let ty = md.1 - (m01 * ms.0 + m00 * ms.1);
    [m00, m01, tx, m01, m00, ty]
}

fn apply2x3(m: &[f32; 6], x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[1] * y + m[2], m[3] * x + m[4] * y + m[5])
}

fn invert2x3(m: &[f32; 6]) -> Option<[f32; 6]> {
    let det = m[0] * m[4] - m[1] * m[3];
    if det.abs() < 1e-12 {
        return None;
    }
    let inv00 = m[4] / det;
    let inv01 = -m[1] / det;
    let inv10 = -m[3] / det;
    let inv11 = m[0] / det;
    Some([
        inv00,
        inv01,
        -(inv00 * m[2] + inv01 * m[5]),
        inv10,
        inv11,
        -(inv10 * m[2] + inv11 * m[5]),
    ])
}

// `Tensor` is re-exported for callers building their own pipelines.
#[allow(unused_imports)]
use Tensor as _TensorAlias;
