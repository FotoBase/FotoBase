//! TASK-01-022 integration: clear-cache while a render job is in flight.
//!
//! "Clear cache files" must empty the thumbnail cache safely: running
//! renders finish or cancel cleanly, the cache ends empty, no partial files
//! (`*.tmp`) survive, and the library itself is untouched.

use std::path::{Path, PathBuf};

use fotobase_jobs::{job, Outcome, Priority, Queue};
use fotobase_lib::thumb::{clear_thumbnail_cache, ThumbOutcome};
use fotobase_render::{thumbnail, RenderError, ThumbOptions};
use image::ImageEncoder as _;

fn gen_jpeg(dir: &Path, i: usize, w: u32, h: u32) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let r = (i * 37 % 251) as u8;
    let g = (i * 91 % 247) as u8;
    let b = (i * 143 % 239) as u8;
    let mut buf = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = ((x + y + i as u32) % 8) as u8;
            buf.extend_from_slice(&[r.wrapping_add(v), g.wrapping_add(v), b.wrapping_add(v)]);
        }
    }
    let mut out = Vec::new();
    {
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
        enc.write_image(&buf, w, h, image::ExtendedColorType::Rgb8).unwrap();
    }
    let p = dir.join(format!("img_{i:05}.jpg"));
    std::fs::write(&p, &out).unwrap();
    p
}

fn entries(cache_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = match std::fs::read_dir(cache_dir) {
        Ok(rd) => rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

#[tokio::test]
async fn clear_empties_a_populated_cache() {
    let src_dir = tempfile::TempDir::new().unwrap();
    let cache_dir = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(src_dir.path(), 0, 64, 48);
    thumbnail(&src, cache_dir.path(), ThumbOptions::default()).expect("render");

    let q: Queue<ThumbOutcome> = Queue::new(4);
    let removed = clear_thumbnail_cache(cache_dir.path(), &q).await.expect("clear");
    assert!(removed >= 1);
    assert!(entries(cache_dir.path()).is_empty(), "cache must end empty");
}

#[tokio::test]
async fn clear_waits_out_an_in_flight_render_then_deletes_everything() {
    let src_dir = tempfile::TempDir::new().unwrap();
    let cache_dir = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(src_dir.path(), 1, 96, 72);

    let q: Queue<ThumbOutcome> = Queue::new(4);
    // The in-flight render: writes its cache entry, then blocks on a gate
    // (simulating the long tail of a decode/encode) before finishing. Clear
    // must cancel + wait for it, then delete its output too.
    let (gate_tx, gate_rx) = tokio::sync::oneshot::channel::<()>();
    let gate_rx = std::sync::Arc::new(tokio::sync::Mutex::new(Some(gate_rx)));
    let target = cache_dir.path().to_path_buf();
    let handle = q
        .try_enqueue(
            "inflight",
            Priority::Visible,
            job(move |ctx| {
                let gate_rx = gate_rx.clone();
                let target = target.clone();
                async move {
                    // Write a "partial" entry the way the real pipeline would
                    // (tmp + rename) while the clear is being requested.
                    let tmp = target.join("inflight.jpg.tmp");
                    std::fs::write(&tmp, b"partial").unwrap();
                    let rx = gate_rx.lock().await.take();
                    if let Some(rx) = rx {
                        // Finish only when the gate opens or we are cancelled —
                        // whichever comes first (cooperative cancellation).
                        let opened = tokio::select! {
                            _ = rx => true,
                            _ = ctx.cancelled() => false,
                        };
                        if opened {
                            std::fs::rename(&tmp, target.join("inflight.jpg")).unwrap();
                        } else {
                            std::fs::remove_file(&tmp).unwrap(); // clean up partial
                        }
                    }
                    Outcome::Done(Err(RenderError::UnsupportedFormat(
                        "simulated in-flight render".into(),
                    )))
                }
            }),
        )
        .expect("enqueue");
    // The sender must stay alive for the whole test: dropping it would
    // resolve the gate immediately and unblock the render.
    std::mem::forget(gate_tx);

    // Request the clear while the render is blocked in flight.
    let clear = {
        let q = q.clone();
        let dir = cache_dir.path().to_path_buf();
        async move { clear_thumbnail_cache(&dir, &q).await }
    };
    let removed = clear.await.expect("clear");
    let _ = removed;
    // Drain cancelled the job before the gate opened; the job cleaned up its
    // partial write, and clear removed whatever remained.
    assert!(matches!(*handle.result().await, Outcome::Cancelled));
    assert!(entries(cache_dir.path()).is_empty(), "no partial files may survive");
    // The library (source) is untouched.
    assert!(src.exists());
}

#[tokio::test]
async fn clear_leaves_the_queue_usable() {
    let src_dir = tempfile::TempDir::new().unwrap();
    let cache_dir = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(src_dir.path(), 2, 64, 48);

    let q: Queue<ThumbOutcome> = Queue::new(4);
    clear_thumbnail_cache(cache_dir.path(), &q).await.expect("clear");

    // After a clear the pipeline keeps working: a fresh render job fills the
    // cache again.
    let h = q
        .try_enqueue(
            "re-render",
            Priority::Visible,
            fotobase_render::thumbnail_job(
                src.clone(),
                cache_dir.path().to_path_buf(),
                Default::default(),
            ),
        )
        .expect("enqueue after clear");
    assert!(matches!(*h.result().await, Outcome::Done(_)));
    assert!(!entries(cache_dir.path()).is_empty(), "cache re-populates after clear");
}

#[tokio::test]
async fn clear_reports_how_many_files_it_removed() {
    let cache_dir = tempfile::TempDir::new().unwrap();
    std::fs::write(cache_dir.path().join("a.jpg"), b"x").unwrap();
    std::fs::write(cache_dir.path().join("b.jpg"), b"x").unwrap();
    std::fs::write(cache_dir.path().join("c.jpg.tmp"), b"x").unwrap();
    std::fs::write(cache_dir.path().join("unrelated.txt"), b"x").unwrap();

    let q: Queue<ThumbOutcome> = Queue::new(4);
    let removed = clear_thumbnail_cache(cache_dir.path(), &q).await.expect("clear");
    assert_eq!(removed, 3, "only thumbnail cache entries are removed");
    assert_eq!(entries(cache_dir.path()), vec!["unrelated.txt".to_string()]);
}
