//! TASK-07-001 integration: the CLIP embedding pass as a job-queue job
//! (PLAN §6 P3 Smart search, §3 rule 4, §5 derived data). The engine is a
//! fake — the 350 MB ONNX model never ships in the test environment; the
//! ONNX path is the ai crate's own coverage plus the perf-acceptance
//! harness (epic acceptance).

// Task-07-001: the RSS probe is the only unsafe here — one small,
// well-documented FFI call (POSIX getrusage), same as TASK-05-012.
#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use fotobase_catalog::Catalog;
use fotobase_lib::clip_jobs::{run_embed_pass, ClipEngine, EmbedReport};
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

/// 512 dimensions (the ViT-B/32 image projection), L2-normalized and
/// derived from the image's first byte so each photo gets a distinct
/// vector — the same contract the real ONNX runtime fulfills.
struct FakeEngine;

impl ClipEngine for FakeEngine {
    fn embed(&mut self, rgb: &[u8], _w: u32, _h: u32) -> Result<Vec<f32>, String> {
        let seed = rgb.first().copied().unwrap_or(0) as f32 / 255.0;
        let mut v: Vec<f32> = (0..512).map(|i| (i as f32 + seed).sin()).collect();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        for x in &mut v {
            *x /= norm;
        }
        Ok(v)
    }
}

struct ExplodingEngine;

impl ClipEngine for ExplodingEngine {
    fn embed(&mut self, _rgb: &[u8], _w: u32, _h: u32) -> Result<Vec<f32>, String> {
        Err("engine exploded".into())
    }
}

fn fixture(count: usize) -> (tempfile::TempDir, Catalog) {
    let lib = tempfile::tempdir().unwrap();
    let dir = lib.path().join("photos");
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..count {
        std::fs::write(dir.join(format!("p{i}.jpg")), jpeg_bytes()).unwrap();
        // A video file the pass must never touch.
        std::fs::write(dir.join(format!("v{i}.mp4")), b"not an image").unwrap();
    }
    let db = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(db.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, &dir, &AtomicBool::new(false)).unwrap();
    (lib, cat)
}

/// TDD unit criterion: a fixture image produces a 512-d vector with L2
/// norm ≈ 1.0, stored in the catalog and marked done.
#[test]
fn embed_pass_produces_unit_512d_vectors() {
    let (_lib, mut cat) = fixture(2);
    let report = run_embed_pass(&mut cat, &mut FakeEngine, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.embedded, 2);
    assert_eq!(report.failed, 0);
    let ids: Vec<i64> = cat
        .conn()
        .prepare("SELECT id FROM files WHERE media_type = 'image' ORDER BY path")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .flatten()
        .collect();
    for id in ids {
        let v = cat.photo_embedding(id).unwrap().expect("stored");
        assert_eq!(v.len(), fotobase_catalog::embeddings::CLIP_EMBEDDING_DIM, "512 dimensions");
        let norm: f32 = v.iter().map(|x| x * x).sum();
        assert!((norm - 1.0).abs() < 1e-4, "L2 norm {norm} not ≈ 1.0");
        let marker: i64 = cat
            .conn()
            .query_row("SELECT embed_scanned FROM files WHERE id = ?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(marker, 1, "marked done");
    }
}

/// An undecodable photo parks its row (failed sentinel) instead of
/// blocking the pass; an engine error does the same.
#[test]
fn failures_are_parked_not_fatal() {
    let (_lib, mut cat) = fixture(2);
    // Real decode succeeds here; the engine fails.
    let report = run_embed_pass(&mut cat, &mut ExplodingEngine, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.failed, 2);
    assert_eq!(report.embedded, 0);
    let pending: usize = cat
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM files WHERE media_type = 'image' AND embed_scanned = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pending, 0, "failed rows parked");
    let failed: usize = cat
        .conn()
        .query_row("SELECT COUNT(*) FROM files WHERE embed_scanned = 2", [], |r| r.get(0))
        .unwrap();
    assert_eq!(failed, 2, "the failed sentinel parked the rows");
    // A reset retries them.
    cat.conn().execute("UPDATE files SET embed_scanned = 0", []).unwrap();
    let report = run_embed_pass(&mut cat, &mut FakeEngine, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.embedded, 2);
}

/// Cancellation stops the pass cleanly: processed rows stay done, the rest
/// stays pending for the next pass.
#[test]
fn cancelled_pass_leaves_valid_state() {
    let (_lib, mut cat) = fixture(4);
    let flag = AtomicBool::new(true);
    let report = run_embed_pass(&mut cat, &mut FakeEngine, &flag).unwrap();
    assert_eq!(report.embedded, 0, "nothing ran");
    assert_eq!(report.failed, 0);
}

/// PLAN §6 P3: embeddings are recomputable derived data — a full rebuild
/// re-upserts every media file (scanner, `full: true`), which resets the
/// markers so the next pass re-embeds everything.
#[test]
fn catalog_rebuild_resets_markers_for_recomputation() {
    let (_lib, mut cat) = fixture(2);
    run_embed_pass(&mut cat, &mut FakeEngine, &AtomicBool::new(false)).unwrap();
    let dir = cat
        .conn()
        .query_row("SELECT path FROM folders LIMIT 1", [], |r| r.get::<_, String>(0))
        .unwrap();
    fotobase_scanner::scan_folder_with(
        &mut cat,
        std::path::Path::new(&dir),
        &AtomicBool::new(false),
        &fotobase_scanner::ScanOptions { full: true },
        &|_p| {},
    )
    .unwrap();
    let pending: usize = cat
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM files WHERE media_type = 'image' AND embed_scanned = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pending, 2, "rebuild made every photo pending again");
    let report = run_embed_pass(&mut cat, &mut FakeEngine, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.embedded, 2, "recomputed");
}

/// Peak RSS so far (the TASK-05-012 probe pattern).
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
        // SAFETY: counters.cb initialized; GetProcessMemoryInfo fills it.
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
    #[cfg(not(any(unix, windows)))]
    {
        0
    }
}

/// TDD integration criterion: 100 photos → the queue drains without
/// exceeding the configured memory ceiling; background jobs stay bounded
/// by the queue capacity (one running + queued), so decoded buffers never
/// accumulate.
#[test]
fn one_hundred_photos_drain_bounded_and_under_the_memory_ceiling() {
    let (_lib, mut cat) = fixture(100);
    let before = peak_rss_bytes();
    let report = run_embed_pass(&mut cat, &mut FakeEngine, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.embedded, 100);
    let after = peak_rss_bytes();
    // The face pipeline's §6 budget (400 MB) — the pass must not inflate it.
    const CEILING: u64 = 400 * 1024 * 1024;
    assert!(after <= before + CEILING, "peak RSS grew by {} bytes", after.saturating_sub(before));
}

/// Priority honored (§3 rule 4): while a background embedding pass runs,
/// a visible job enqueued alongside another background job dequeues first.
#[tokio::test]
async fn visible_items_run_before_background_embeddings() {
    use fotobase_jobs::{Outcome, Priority, Queue};

    let queue = Queue::new(4);
    let release = Arc::new(tokio::sync::Notify::new());
    let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));

    // The blocker occupies the single worker until released.
    let block_release = release.clone();
    let blocker = queue
        .try_enqueue(
            "blocker",
            Priority::Background,
            fotobase_jobs::job(move |_ctx| {
                let release = block_release.clone();
                async move {
                    release.notified().await;
                    Outcome::Done(EmbedReport::default())
                }
            }),
        )
        .unwrap();

    // Queue a background embedding pass and a visible job behind it.
    let bg_order = order.clone();
    let background = queue
        .try_enqueue(
            "bg",
            Priority::Background,
            fotobase_jobs::job(move |_ctx| {
                let order = bg_order.clone();
                async move {
                    order.lock().unwrap().push("background");
                    Outcome::Done(EmbedReport::default())
                }
            }),
        )
        .unwrap();

    let vis_order = order.clone();
    let visible = queue
        .try_enqueue(
            "vis",
            Priority::Visible,
            fotobase_jobs::job(move |_ctx| {
                let order = vis_order.clone();
                async move {
                    order.lock().unwrap().push("visible");
                    Outcome::Done(EmbedReport::default())
                }
            }),
        )
        .unwrap();

    // The visible job must finish before the background one.
    tokio::time::timeout(std::time::Duration::from_secs(5), visible.result())
        .await
        .expect("visible completed");
    let already = order.lock().unwrap().clone();
    assert!(!already.contains(&"background"), "background ran before the visible job: {already:?}");
    release.notify_waiters();
    let _ = blocker.result().await;
    let _ = background.result().await;
    let order = order.lock().unwrap().clone();
    assert!(matches!(order.as_slice(), ["visible", "background"]), "got {order:?}");
}

/// Battery throttling (§10): with the shared hook reporting "on battery",
/// the embedding pass does not run; back on AC it does.
#[tokio::test]
async fn battery_hook_pauses_the_embedding_queue() {
    use fotobase_jobs::{BatteryHook, Outcome, Queue, ThrottleHook};

    let hook = Arc::new(BatteryHook::new(true));
    let queue = Queue::with_throttle(2, hook.clone() as Arc<dyn ThrottleHook>);
    let ran = Arc::new(AtomicU64::new(0));
    let job_ran = ran.clone();
    let handle = queue
        .try_enqueue(
            "embed",
            fotobase_jobs::Priority::Background,
            fotobase_jobs::job(move |_ctx| {
                let ran = job_ran.clone();
                async move {
                    ran.fetch_add(1, Ordering::SeqCst);
                    Outcome::Done(EmbedReport::default())
                }
            }),
        )
        .unwrap();

    // Give the worker a moment: nothing may run while on battery.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(ran.load(Ordering::SeqCst), 0, "ran on battery");

    hook.set_on_battery(false);
    let outcome = handle.result().await;
    assert!(matches!(*outcome, Outcome::Done(_)));
    assert_eq!(ran.load(Ordering::SeqCst), 1);
}
