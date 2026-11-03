//! Face detection + embedding passes as job-queue jobs (TASK-05-002,
//! PLAN §6 P3 People, §3 rule 4, §10 battery).
//!
//! A full-library face pass walks only the folders whose per-folder
//! face-detection toggle (TASK-05-010, `folders.face_detect`) is on — a
//! disabled folder schedules nothing and its existing faces are left alone.
//! Detection and embedding are separate engine calls so the embedding step
//! skips already-processed faces (`files.face_scanned` marks 0 pending /
//! 1 done / 2 failed, the phash sentinel pattern). Everything written is
//! derived data in the catalog (PLAN §5) — faces are never written to
//! sidecars from this path (MWG writes are TASK-05-008, sidecar rules).
//!
//! [`FaceEngine`] abstracts the ONNX runtime so every scheduling and
//! catalog rule is testable without models; [`OrtEngine`] is the
//! production wrapper over `fotobase_ai::FaceRuntime`. Images decode to
//! in-memory RGB through `fotobase_render::decode_rgb` — no image bytes
//! ever travel over IPC (PLAN §3 rule 3).
//!
//! Battery: the queue this module runs on is built with a
//! [`fotobase_jobs::BatteryHook`]; [`probe_on_battery`] feeds it
//! best-effort (Linux sysfs, macOS pmset; unknown → AC, never guessing a
//! pause). No telemetry, no network.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use fotobase_ai::{align_face, FaceRuntime};
use fotobase_catalog::{Catalog, FaceRecord};
use serde::Serialize;

/// Tauri event the face pass reports progress on (§3: plain counters, no
/// image bytes).
pub const FACES_EVENT: &str = "faces-progress";

/// How often the app re-probes the power state for the battery hook.
pub const PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// One full pass over the unprocessed images of the face-enabled folders.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FaceReport {
    /// Faces detected on this pass.
    pub detected: usize,
    /// Faces embedded on this pass.
    pub embedded: usize,
    /// Photos that could not be decoded (marked failed; a later explicit
    /// reset retries them).
    pub failed: usize,
    /// Catalog folders the pass left alone because their toggle is off.
    pub folders_skipped: usize,
}

/// The detection + embedding backend. Separate methods (not one
/// "process photo") so an embedding-only retry can skip detection later
/// without re-detecting.
pub trait FaceEngine: Send {
    fn detect(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<fotobase_ai::DetectedFace>, String>;
    fn embed(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
        landmarks: &[f32; 10],
    ) -> Result<Vec<f32>, String>;
}

/// Production engine: YuNet detection + SFace-style embedding on the CPU
/// ort runtime (TASK-05-001 models, opt-in downloads).
pub struct OrtEngine {
    runtime: FaceRuntime,
}

impl OrtEngine {
    /// Load both models from the catalog location's model directory (the
    /// registry from 05-001 owns checksums and installation).
    pub fn load(model_dir: &Path) -> Result<Self, String> {
        let detector = model_dir.join("face_detection_yunet_2023mar.onnx");
        let embedder = model_dir.join("face_recognition_sface_2021dec.onnx");
        let runtime = FaceRuntime::load(&detector, &embedder).map_err(|e| e.to_string())?;
        Ok(Self { runtime })
    }
}

impl FaceEngine for OrtEngine {
    fn detect(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<fotobase_ai::DetectedFace>, String> {
        self.runtime.detect_faces(rgb, width, height).map_err(|e| e.to_string())
    }

    fn embed(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
        landmarks: &[f32; 10],
    ) -> Result<Vec<f32>, String> {
        let crop = align_face(rgb, width, height, landmarks).map_err(|e| e.to_string())?;
        self.runtime.embed_face(&crop, 112, 112).map_err(|e| e.to_string())
    }
}

/// Best-effort power probe for the battery hook: `Some(true)` = on
/// battery, `Some(false)` = on AC, `None` = unknown (the caller treats
/// unknown as AC — never pausing on a guess). Linux reads sysfs; macOS
/// parses `pmset -g batt`; elsewhere `None`.
pub fn probe_on_battery() -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
            for entry in entries.flatten() {
                let status = entry.path().join("status");
                if let Ok(state) = std::fs::read_to_string(&status) {
                    match state.trim() {
                        "Discharging" => return Some(true),
                        "Charging" | "Full" | "Not charging" | "Unknown" => return Some(false),
                        _ => {}
                    }
                }
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("pmset").arg("-g").arg("batt").output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        Some(text.contains("discharging"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// One full-library face pass. Cooperative cancellation: a cancelled pass
/// leaves valid state — unprocessed rows stay pending, a later pass picks
/// them up; a photo whose detection already ran is never re-detected.
pub fn run_face_pass(
    cat: &mut Catalog,
    engine: &mut dyn FaceEngine,
    cancel: &AtomicBool,
) -> Result<FaceReport, String> {
    let mut report = FaceReport::default();

    // TASK-05-010: the toggle is read before anything is scheduled.
    let enabled = cat.face_enabled_folder_ids().map_err(|e| e.to_string())?;
    let total_folders: usize = cat
        .conn()
        .query_row("SELECT COUNT(*) FROM folders", [], |r| r.get::<_, i64>(0))
        .map_err(|e| e.to_string())? as usize;
    report.folders_skipped = total_folders.saturating_sub(enabled.len());
    if enabled.is_empty() || cancel.load(Ordering::Relaxed) {
        return Ok(report);
    }

    let batch = cat.face_candidates(&enabled).map_err(|e| e.to_string())?;
    for (file_id, path) in batch {
        if cancel.load(Ordering::Relaxed) {
            return Ok(report);
        }
        let rgb = match fotobase_render::decode_rgb(Path::new(&path)) {
            Ok((rgb, w, h)) => (rgb, w, h),
            Err(_) => {
                // Park the row (sentinel) so one undecodable photo neither
                // blocks the pass forever nor inflates later counts.
                cat.mark_face_scanned(file_id, false).map_err(|e| e.to_string())?;
                report.failed += 1;
                continue;
            }
        };
        let faces = match engine.detect(&rgb.0, rgb.1, rgb.2) {
            Ok(faces) => faces,
            Err(_) => {
                cat.mark_face_scanned(file_id, false).map_err(|e| e.to_string())?;
                report.failed += 1;
                continue;
            }
        };
        for face in &faces {
            let vector = engine.embed(&rgb.0, rgb.1, rgb.2, &face.landmarks).ok();
            if vector.is_some() {
                report.embedded += 1;
            }
            let embedded = vector.as_deref();
            let record = FaceRecord {
                x: face.x,
                y: face.y,
                width: face.width,
                height: face.height,
                landmarks: face.landmarks,
                confidence: face.confidence,
            };
            cat.record_face(file_id, &record, embedded).map_err(|e| e.to_string())?;
            report.detected += 1;
        }
        cat.mark_face_scanned(file_id, true).map_err(|e| e.to_string())?;
    }
    Ok(report)
}

/// Monotonic sequence for face-pass job keys (one full pass at a time
/// coalesces; a per-folder trigger while a pass runs attaches to it).
static PASS_SEQ: AtomicU64 = AtomicU64::new(0);

/// Job key for the next full-library pass.
pub fn job_key() -> String {
    format!("face-pass-{}", PASS_SEQ.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_identity() {
        // The lib.rs smoke test list expects every module to be reachable.
        assert!(!FACES_EVENT.is_empty());
    }
}
