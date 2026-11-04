//! TASK-05-012: resource-cap tests for the face pipeline on battery
//! (PLAN §9 AI row, §6 P3 acceptance, §3 rule 4). Two caps:
//!
//! 1. **Throttling** — with the shared power state reporting "on
//!    battery", the face pass enqueued on the app's queue must not run at
//!    all (the queue is built with a [`fotobase_jobs::BatteryHook`], the
//!    same wiring `face_jobs.rs` documents); it runs once back on AC.
//! 2. **Memory ceiling** — a full pass over a real photo batch (decoded
//!    RGB buffers through the production `run_face_pass` path) must keep
//!    process peak RSS at or below the §6 P3 budget of 400 MB.

// Task-05-012: the RSS probes are the only unsafe here — two small,
// well-documented FFI calls (POSIX getrusage, Win32 GetProcessMemoryInfo).
#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use fotobase_catalog::Catalog;
use fotobase_lib::face_jobs::{run_face_pass, FaceEngine, FaceReport};
use fotobase_scanner::scan_folder;

/// Peak RSS so far, in bytes. `getrusage(RUSAGE_SELF).ru_maxrss` on
/// Unix (KB on Linux, bytes on macOS — both normalized), the working-set
/// peak on Windows. Monotonic process-lifetime peak: the ceiling
/// assertion is against the value after the pipeline ran.
fn peak_rss_bytes() -> u64 {
    #[cfg(unix)]
    {
        #[cfg(target_os = "macos")]
        const KIB: u64 = 1;
        #[cfg(not(target_os = "macos"))]
        const KIB: u64 = 1024;
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        // SAFETY: getrusage with an rusage-sized buffer, per POSIX.
        let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
        assert_eq!(rc, 0, "getrusage failed");
        let usage = unsafe { usage.assume_init() };
        usage.ru_maxrss as u64 * KIB
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        // GetCurrentProcess returns a pseudo-handle that needs no closing
        // and is valid for query APIs in the calling process.
        let handle = unsafe { GetCurrentProcess() };
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            PageFaultCount: 0,
            PeakWorkingSetSize: 0,
            WorkingSetSize: 0,
            QuotaPeakPagedPoolUsage: 0,
            QuotaPagedPoolUsage: 0,
            QuotaPeakNonPagedPoolUsage: 0,
            QuotaNonPagedPoolUsage: 0,
            PagefileUsage: 0,
            PeakPagefileUsage: 0,
        };
        let ok = unsafe {
            GetProcessMemoryInfo(
                handle as _,
                &mut counters,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            )
        };
        assert!(ok != 0, "GetProcessMemoryInfo failed");
        counters.PeakWorkingSetSize as u64
    }
}

/// A tiny valid JPEG, `width × height`, solid color.
fn jpeg_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut img = image::RgbImage::new(width, height);
    for (_, _, p) in img.enumerate_pixels_mut() {
        *p = image::Rgb([180, 160, 140]);
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .unwrap();
    out
}

/// Deterministic fake engine (no ONNX models in the test environment —
/// same criterion as `face_jobs.rs`): one face per photo, a 128-d
/// embedding derived from the image bytes.
struct FakeEngine;

impl FaceEngine for FakeEngine {
    fn detect(
        &mut self,
        _rgb: &[u8],
        _w: u32,
        _h: u32,
    ) -> Result<Vec<fotobase_ai::DetectedFace>, String> {
        Ok(vec![fotobase_ai::DetectedFace {
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
        Ok((0..128)
            .map(|i| {
                if i == 0 {
                    seed
                } else if i == 1 {
                    1.0 - seed
                } else {
                    0.5
                }
            })
            .collect())
    }
}

/// A library of `count` real JPEGs (1024×768) scanned into a fresh
/// catalog, face detection enabled on the folder.
fn fixture(count: usize) -> (tempfile::TempDir, Catalog) {
    let lib = tempfile::tempdir().unwrap();
    let dir = lib.path().join("photos");
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..count {
        std::fs::write(dir.join(format!("p{i}.jpg")), jpeg_bytes(1024, 768)).unwrap();
    }
    let db = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(db.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, &dir, &AtomicBool::new(false)).unwrap();
    cat.set_folder_face_detection(dir.to_str().unwrap(), true).unwrap();
    (lib, cat)
}

/// PLAN §1/§6: the face pipeline on 10k photos stays ≤ 400 MB.
const MEMORY_CEILING_BYTES: u64 = 400 * 1024 * 1024;

#[test]
fn face_pass_keeps_peak_rss_under_the_400mb_ceiling() {
    // Enough real decodes to exercise the pipeline's buffers (32 ×
    // 1024×768 RGB ≈ 75 MB of decoded data flowing through) without
    // dominating CI wall time.
    let (_lib, mut cat) = fixture(32);
    let before = peak_rss_bytes();
    let report = run_face_pass(&mut cat, &mut FakeEngine, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.detected, 32);
    let after = peak_rss_bytes();
    println!(
        "face pass peak RSS: {after} bytes (before {before}) — ceiling {MEMORY_CEILING_BYTES}"
    );
    assert!(
        after <= MEMORY_CEILING_BYTES,
        "peak RSS {after} exceeds the 400 MB resource cap ({MEMORY_CEILING_BYTES})"
    );
}

/// §3 rule 4 on battery: the pass sits in the queue, runs nothing, until
/// the power state flips back to AC.
#[tokio::test]
async fn face_pass_is_throttled_while_on_battery() {
    use fotobase_jobs::{BatteryHook, Outcome, Priority, Queue, ThrottleHook};

    let (_lib, cat) = fixture(2);
    let hook = Arc::new(BatteryHook::new(true));
    let queue = Queue::with_throttle(2, hook.clone() as Arc<dyn ThrottleHook>);
    let ran = Arc::new(AtomicU64::new(0));
    let ran_in_job = ran.clone();
    let job_cat = Arc::new(Mutex::new(cat));
    let handle = queue
        .try_enqueue(
            "face-pass-0",
            Priority::Background,
            fotobase_jobs::job(move |_ctx| {
                let ran_job = ran_in_job.clone();
                let cat = job_cat.clone();
                async move {
                    ran_job.fetch_add(1, Ordering::SeqCst);
                    let mut cat = cat.lock().unwrap();
                    match run_face_pass(
                        &mut cat,
                        &mut FakeEngine,
                        &AtomicBool::new(_ctx.is_cancelled()),
                    ) {
                        Ok(report) => Outcome::Done(report),
                        Err(e) => Outcome::Failed(e),
                    }
                }
            }),
        )
        .unwrap();

    // The queue is parked on battery; give the worker a beat and confirm
    // the job never started.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(ran.load(Ordering::SeqCst), 0, "face pass ran while on battery");

    // Back on AC: the parked job runs and the pipeline completes.
    hook.set_on_battery(false);
    let outcome = handle.result().await;
    assert!(matches!(*outcome, Outcome::Done(FaceReport { detected: 2, .. })));
    assert_eq!(ran.load(Ordering::SeqCst), 1);
}
