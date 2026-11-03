//! TASK-05-002 / TASK-05-010 integration: the face pass as a job-queue job,
//! honoring the per-folder face-detection toggle (PLAN §6 P3 People, §3
//! rule 4, §5 derived data). The engine is a fake — no ONNX models in the
//! test environment; the ONNX path is the ai crate's own coverage plus the
//! perf-acceptance harness (10k-fixture overnight budget, epic acceptance).

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use fotobase_ai::DetectedFace;
use fotobase_catalog::Catalog;
use fotobase_lib::face_jobs::{run_face_pass, FaceEngine, FaceReport};
use fotobase_scanner::scan_folder;

/// A tiny valid JPEG (solid color) the real decoder accepts.
fn jpeg_bytes() -> Vec<u8> {
    let mut img = image::RgbImage::new(64, 48);
    for (_, _, p) in img.enumerate_pixels_mut() {
        *p = image::Rgb([200, 180, 160]);
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .unwrap();
    out
}

/// Deterministic fake: one face per photo, embedding derived from the
/// image's first bytes so each photo gets a distinct vector.
struct FakeEngine {
    calls: usize,
}

impl FaceEngine for FakeEngine {
    fn detect(&mut self, _rgb: &[u8], _w: u32, _h: u32) -> Result<Vec<DetectedFace>, String> {
        self.calls += 1;
        Ok(vec![DetectedFace {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 24.0,
            landmarks: [0.0; 10],
            confidence: 0.99,
        }])
    }

    fn embed(
        &mut self,
        rgb: &[u8],
        _w: u32,
        _h: u32,
        _landmarks: &[f32; 10],
    ) -> Result<Vec<f32>, String> {
        let seed = rgb.first().copied().unwrap_or(0) as f32 / 255.0;
        Ok(vec![seed, 1.0 - seed, 0.5])
    }
}

/// Two folders, one photo each, scanned into a fresh catalog. Returns the
/// two on-disk folder paths (the catalog stores full paths) and the two
/// file ids ordered by path (a.jpg in /on, b.jpg in /off).
fn fixture() -> (tempfile::TempDir, Catalog, String, String, i64, i64) {
    let lib = tempfile::tempdir().unwrap();
    let on_dir = lib.path().join("on");
    let off_dir = lib.path().join("off");
    std::fs::create_dir_all(&on_dir).unwrap();
    std::fs::create_dir_all(&off_dir).unwrap();
    std::fs::write(on_dir.join("a.jpg"), jpeg_bytes()).unwrap();
    std::fs::write(off_dir.join("b.jpg"), jpeg_bytes()).unwrap();
    let db = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(db.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, &on_dir, &AtomicBool::new(false)).unwrap();
    scan_folder(&mut cat, &off_dir, &AtomicBool::new(false)).unwrap();
    let on_path = on_dir.to_str().unwrap().to_string();
    let off_path = off_dir.to_str().unwrap().to_string();
    // ids ordered by folder path (off/b.jpg sorts before on/a.jpg).
    let ids: Vec<(i64, String)> = cat
        .conn()
        .prepare(
            "SELECT f.id, fo.path FROM files f JOIN folders fo ON fo.id = f.folder_id \
             ORDER BY fo.path, f.path",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(ids.len(), 2);
    let (b, off_stored) = &ids[0];
    let (a, on_stored) = &ids[1];
    assert_eq!(off_stored, &off_path);
    assert_eq!(on_stored, &on_path);
    (lib, cat, on_path, off_path, *a, *b)
}

#[test]
fn disabled_folders_are_skipped_by_the_full_library_pass() {
    let (_lib, mut cat, _on, _off, _a, _b) = fixture();
    // Both toggles off (the default): a full-library pass does nothing,
    // touches no file, and leaves every file pending.
    let report =
        run_face_pass(&mut cat, &mut FakeEngine { calls: 0 }, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.detected, 0);
    assert_eq!(report.folders_skipped, 2);
    let pending: usize = cat
        .conn()
        .query_row("SELECT COUNT(*) FROM files WHERE face_scanned = 0", [], |r| r.get(0))
        .unwrap();
    assert_eq!(pending, 2, "disabled folders must stay untouched");
}

#[test]
fn enabled_folders_are_processed_and_the_toggle_gates_scheduling() {
    let (_lib, mut cat, on, _off, a, b) = fixture();
    cat.set_folder_face_detection(&on, true).unwrap();

    let report =
        run_face_pass(&mut cat, &mut FakeEngine { calls: 0 }, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.detected, 1, "only the enabled folder's photo processed");
    assert_eq!(report.embedded, 1);
    assert_eq!(report.folders_skipped, 1);

    let rows = cat.faces_for_file(a).unwrap();
    assert_eq!(rows.len(), 1);
    let (_, vector) = cat.face_embedding(rows[0].id).unwrap().unwrap();
    assert_eq!(vector.len(), 3);

    // The disabled folder's photo is untouched.
    assert!(cat.faces_for_file(b).unwrap().is_empty());
}

#[test]
fn toggling_on_later_triggers_detection_for_that_folder() {
    let (_lib, mut cat, on, off, a, b) = fixture();
    // First pass with everything off: nothing happens.
    run_face_pass(&mut cat, &mut FakeEngine { calls: 0 }, &AtomicBool::new(false)).unwrap();

    // The user flips the toggle for the off folder later; the next pass
    // picks it up.
    cat.set_folder_face_detection(&off, true).unwrap();
    let report =
        run_face_pass(&mut cat, &mut FakeEngine { calls: 0 }, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.detected, 1);
    assert!(!cat.faces_for_file(b).unwrap().is_empty());
    assert!(cat.faces_for_file(a).unwrap().is_empty());

    // And only the newly enabled folder's photo: the first pass never
    // marked the on folder's photo, so this pass processes exactly it.
    cat.set_folder_face_detection(&on, true).unwrap();
    let report =
        run_face_pass(&mut cat, &mut FakeEngine { calls: 0 }, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.detected, 1);
    assert!(!cat.faces_for_file(a).unwrap().is_empty());
}

#[test]
fn a_second_pass_skips_already_processed_files() {
    let (_lib, mut cat, _on, _off, _a, _b) = fixture();
    cat.set_folder_face_detection("/on", true).unwrap();
    run_face_pass(&mut cat, &mut FakeEngine { calls: 0 }, &AtomicBool::new(false)).unwrap();

    let mut engine = FakeEngine { calls: 0 };
    let report = run_face_pass(&mut cat, &mut engine, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.detected, 0, "embeds skip already-processed faces");
    assert_eq!(engine.calls, 0);
}

#[test]
fn cancelled_pass_stops_between_photos() {
    let (_lib, mut cat, _on, _off, _a, _b) = fixture();
    cat.set_folder_face_detection("/on", true).unwrap();
    cat.set_folder_face_detection("/off", true).unwrap();
    let report =
        run_face_pass(&mut cat, &mut FakeEngine { calls: 0 }, &AtomicBool::new(true)).unwrap();
    assert_eq!(report.detected, 0);
}

/// The queue form (§3 rule 4): same outcome through a bounded queue with the
/// battery hook installed, mirroring how the app wires it.
#[tokio::test]
async fn face_pass_runs_through_the_job_queue() {
    use fotobase_jobs::{BatteryHook, Outcome, Priority, Queue, ThrottleHook};

    let (_lib, cat, _on, _off, _a, _b) = fixture();
    let hook = Arc::new(BatteryHook::new(false));
    let queue = Queue::with_throttle(2, hook.clone() as Arc<dyn ThrottleHook>);
    let cat = Arc::new(Mutex::new(cat));
    let job_cat = cat.clone();
    let handle = queue
        .try_enqueue(
            "face-pass-0",
            Priority::Background,
            fotobase_jobs::job(move |_ctx| {
                let cat = job_cat.clone();
                async move {
                    let mut cat = cat.lock().unwrap();
                    match run_face_pass(
                        &mut cat,
                        &mut FakeEngine { calls: 0 },
                        &AtomicBool::new(_ctx.is_cancelled()),
                    ) {
                        Ok(report) => Outcome::Done(report),
                        Err(e) => Outcome::Failed(e),
                    }
                }
            }),
        )
        .unwrap();
    let outcome = handle.result().await;
    assert!(matches!(*outcome, Outcome::Done(FaceReport { detected: 0, .. })));
}
