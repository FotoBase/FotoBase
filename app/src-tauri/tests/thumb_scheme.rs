//! `thumb://` URI scheme handler tests (TASK-01-011, PLAN §3 rule 3).
//!
//! The scheme serves cached thumbnails straight from the disk cache: zero
//! image bytes cross IPC. A cache miss returns a placeholder (and triggers a
//! render job), never an error dialog.
//!
//! The resolution logic lives in [`fotobase_lib::thumb`], a pure module that
//! is testable without booting a Tauri app; the Tauri registration in
//! `lib.rs` is a thin adapter over it.

use std::path::{Path, PathBuf};

use fotobase_jobs::Outcome;
use fotobase_lib::thumb::{
    enqueue_missing_thumbs, hash_from_uri, resolve_thumb, ThumbOutcome, ThumbRequest,
    PLACEHOLDER_PNG,
};
use fotobase_render::{thumbnail, ThumbOptions};
use image::ImageEncoder as _;
use tauri::http::Uri;
use tempfile::TempDir;

/// Deterministic tiny JPEG generator (same recipe as the render-crate tests).
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

/// The content hash the render pipeline writes: first 16 bytes of SHA-256,
/// lowercase hex (32 chars).
fn cache_key(src: &Path) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let bytes = std::fs::read(src).unwrap();
    Sha256::digest(&bytes)[..16].iter().fold(String::new(), |mut acc, b| {
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// 1x cache request (the legacy form, no `?s=` query).
fn req1(hash: &str) -> ThumbRequest {
    ThumbRequest { hash: hash.to_string(), scale: 1 }
}

// --- URI parsing -----------------------------------------------------------

#[test]
fn hash_from_uri_accepts_the_direct_scheme_form() {
    // macOS/Linux WebView: `thumb://<hash>` — the hash is the authority.
    let uri: Uri = "thumb://e3b0c44298fc1c149afbf4c8996fb924".parse().unwrap();
    assert_eq!(hash_from_uri(&uri).as_deref(), Some("e3b0c44298fc1c149afbf4c8996fb924"));
}

#[test]
fn hash_from_uri_accepts_the_windows_webview_form() {
    // Windows WebView2 serves custom schemes as `http://<scheme>.localhost/…`.
    let uri: Uri = "http://thumb.localhost/e3b0c44298fc1c149afbf4c8996fb924".parse().unwrap();
    assert_eq!(hash_from_uri(&uri).as_deref(), Some("e3b0c44298fc1c149afbf4c8996fb924"));

    // `thumb://localhost/<hash>` must also parse.
    let uri: Uri = "thumb://localhost/e3b0c44298fc1c149afbf4c8996fb924".parse().unwrap();
    assert_eq!(hash_from_uri(&uri).as_deref(), Some("e3b0c44298fc1c149afbf4c8996fb924"));
}

#[test]
fn hash_from_uri_strips_query_and_fragment() {
    let uri: Uri = "thumb://e3b0c44298fc1c149afbf4c8996fb924?v=2".parse().unwrap();
    assert_eq!(hash_from_uri(&uri).as_deref(), Some("e3b0c44298fc1c149afbf4c8996fb924"));
}

#[test]
fn hash_from_uri_rejects_bad_input() {
    // Not exactly 32 lowercase hex chars, traversal attempts, wrong host:
    // anything that is not a well-formed cache key is rejected (the caller
    // then serves the placeholder — never an error dialog).
    for bad in [
        "thumb://",                                               // empty
        "thumb://e3b0c44298fc1c149afbf4c8996fb92",                // 31 chars
        "thumb://e3b0c44298fc1c149afbf4c8996fb9244",              // 33 chars
        "thumb://E3B0C44298FC1C149AFBF4C8996FB924",               // uppercase
        "thumb://zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",               // non-hex
        "thumb://../../etc/passwd",                               // traversal
        "http://thumb.localhost/../../etc/passwd",                // traversal, windows form
        "http://evil.localhost/e3b0c44298fc1c149afbf4c8996fb924", // wrong scheme host
    ] {
        // An input the HTTP layer cannot even parse is rejected by definition;
        // one that parses must never yield a cache key.
        let outcome = bad.parse::<Uri>().ok().as_ref().and_then(hash_from_uri);
        assert!(outcome.is_none(), "{bad} must not parse");
    }
}

// --- Resolution ------------------------------------------------------------

#[test]
fn cached_hash_returns_the_cached_bytes() {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("thumbs");
    let src = gen_jpeg(tmp.path(), 1, 120, 80);
    let thumb = thumbnail(&src, &cache, ThumbOptions::default()).unwrap();
    let expected = std::fs::read(&thumb.path).unwrap();

    let reply = resolve_thumb(&req1(&cache_key(&src)), &cache);

    assert_eq!(reply.status, 200);
    assert_eq!(reply.content_type, "image/jpeg");
    assert_eq!(reply.body, expected, "the cached bytes are served verbatim");
}

#[test]
fn unknown_hash_returns_the_placeholder_never_an_error() {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("thumbs"); // empty cache → guaranteed miss

    let reply = resolve_thumb(&req1("e3b0c44298fc1c149afbf4c8996fb924"), &cache);

    assert_eq!(reply.status, 200, "a miss must still be a displayable image");
    assert_eq!(reply.content_type, "image/png");
    assert_eq!(reply.body, PLACEHOLDER_PNG);
}

#[test]
fn unreadable_cache_entry_degrades_to_placeholder() {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("thumbs");
    let src = gen_jpeg(tmp.path(), 1, 60, 60);
    thumbnail(&src, &cache, ThumbOptions::default()).unwrap();
    let hash = cache_key(&src);

    // Make the cached entry present but unreadable, portably: a directory
    // where the file belongs. `fs::read` then fails on every OS (EISDIR on
    // Unix, ERROR_ACCESS_DENIED on Windows) — the IO-error path, not the
    // plain miss path.
    let entry = cache.join(format!("{hash}.jpg"));
    std::fs::remove_file(&entry).unwrap();
    std::fs::create_dir(&entry).unwrap();

    let reply = resolve_thumb(&req1(&hash), &cache);
    assert_eq!(reply.status, 200);
    assert_eq!(reply.content_type, "image/png");
    assert_eq!(reply.body, PLACEHOLDER_PNG);
}

#[test]
fn the_placeholder_is_a_valid_png_image() {
    let img = image::load_from_memory(PLACEHOLDER_PNG).expect("placeholder must decode");
    assert!(img.width() > 0 && img.height() > 0);
}

// --- Cache-miss render job trigger -----------------------------------------

mod miss_jobs {
    use super::*;
    use fotobase_catalog::Catalog;
    use fotobase_jobs::Queue;

    fn catalog_with_hash(dir: &TempDir, hash: &str, path: &str) -> Catalog {
        let mut cat = Catalog::open(dir.path().join("catalog.db")).unwrap();
        let folder = cat.seed_folder("/photos").unwrap();
        cat.conn()
            .execute(
                "INSERT INTO files(folder_id, path, filename, size, mtime, hash, media_type)
                 VALUES (?1, ?2, ?3, 1, 0.0, ?4, 'image')",
                rusqlite::params![folder, path, "unused", Some(hash)],
            )
            .unwrap();
        cat
    }

    #[tokio::test]
    async fn cache_miss_enqueues_a_render_job_for_the_hashed_source() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("thumbs");
        let src = gen_jpeg(&tmp.path().join("photos"), 1, 200, 150);
        let src_str = src.to_string_lossy().to_string();
        let cat = catalog_with_hash(&tmp, "e3b0c44298fc1c149afbf4c8996fb924", &src_str);

        let queue: Queue<ThumbOutcome> = Queue::new(8);
        let handles = enqueue_missing_thumbs(
            &req1("e3b0c44298fc1c149afbf4c8996fb924"),
            &cat,
            &cache,
            &queue,
            None,
            None,
        );
        assert_eq!(handles.len(), 1, "one source path → one render job");

        // The job renders into the cache, making the next request a hit.
        for handle in handles {
            match &*handle.result().await {
                Outcome::Done(Ok(_)) => {}
                other => panic!("render job must succeed, got {other:?}"),
            }
        }
        // The render caches under the source's real content hash.
        let reply = resolve_thumb(&req1(&cache_key(&src)), &cache);
        assert_eq!(reply.content_type, "image/jpeg", "render job filled the cache");
        queue.close().await;
    }

    #[tokio::test]
    async fn miss_for_unknown_hash_enqueues_nothing() {
        let tmp = TempDir::new().unwrap();
        let cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
        let queue: Queue<ThumbOutcome> = Queue::new(8);
        let handles = enqueue_missing_thumbs(
            &req1("e3b0c44298fc1c149afbf4c8996fb924"),
            &cat,
            &tmp.path().join("thumbs"),
            &queue,
            None,
            None,
        );
        assert!(handles.is_empty());
        queue.close().await;
    }

    #[tokio::test]
    async fn duplicate_requests_coalesce_onto_one_job() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("thumbs");
        let src = gen_jpeg(&tmp.path().join("photos"), 2, 100, 100);
        let cat =
            catalog_with_hash(&tmp, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", &src.to_string_lossy());

        let queue: Queue<ThumbOutcome> = Queue::new(8);
        let key = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let first = enqueue_missing_thumbs(&req1(key), &cat, &cache, &queue, None, None);
        let second = enqueue_missing_thumbs(&req1(key), &cat, &cache, &queue, None, None);
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1, "second request coalesces onto the same job");

        let all: Vec<_> = first.into_iter().chain(second).collect();
        // Both handles share one outcome; the job itself ran exactly once and
        // filled the cache.
        for handle in all {
            match &*handle.result().await {
                Outcome::Done(Ok(_)) => {}
                other => panic!("coalesced render job must succeed, got {other:?}"),
            }
        }
        assert_eq!(resolve_thumb(&req1(&cache_key(&src)), &cache).content_type, "image/jpeg");
        queue.close().await;
    }

    #[tokio::test]
    async fn display_profile_renders_into_a_profile_tagged_cache_entry() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("thumbs");
        let src = gen_jpeg(&tmp.path().join("photos"), 3, 120, 90);
        let src_str = src.to_string_lossy().to_string();
        let cat = catalog_with_hash(&tmp, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", &src_str);
        let queue: Queue<ThumbOutcome> = Queue::new(8);

        // The same content hash rendered for two different display profiles
        // must land in two distinct cache entries, neither of them the
        // legacy untagged one (TASK-02-001 integration criterion).
        let p3 = fotobase_render::rgb_profile_bytes(
            [0.3127, 0.329, 1.0],
            [[0.680, 0.320, 1.0], [0.265, 0.690, 1.0], [0.150, 0.060, 1.0]],
            2.199_218_75,
        )
        .unwrap();
        let srgb = fotobase_render::srgb_profile_bytes().unwrap();

        for profile in [Some(p3.clone()), Some(srgb.clone()), None] {
            let handles = enqueue_missing_thumbs(
                &req1("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
                &cat,
                &cache,
                &queue,
                profile,
                None,
            );
            assert_eq!(handles.len(), 1);
            for handle in handles {
                match &*handle.result().await {
                    Outcome::Done(Ok(t)) => {
                        let name = t.path.file_name().unwrap().to_str().unwrap().to_string();
                        assert!(
                            name.starts_with(&cache_key(&src)),
                            "keyed by content hash: {name}"
                        );
                    }
                    other => panic!("render must succeed, got {other:?}"),
                }
            }
        }
        // Three distinct entries: p3-tagged, srgb-tagged, legacy.
        let entries: Vec<String> = std::fs::read_dir(&cache)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".jpg"))
            .collect();
        assert_eq!(entries.len(), 3, "one cache entry per display profile: {entries:?}");
        queue.close().await;
    }
}

// --- video thumbnails (TASK-02-004) ----------------------------------------

mod video_thumbs {
    use super::*;
    use fotobase_catalog::Catalog;
    use fotobase_jobs::{Outcome, Priority, Queue};
    use fotobase_video::FrameExtractor;
    use std::path::Path;
    use std::sync::Arc;

    /// Fake extractor: writes a tiny decodable JPEG as the "extracted frame".
    struct FakeOk;
    impl FrameExtractor for FakeOk {
        fn extract_frame(&self, _v: &Path, out: &Path) -> Result<(), fotobase_video::VideoError> {
            let mut buf = Vec::new();
            for _ in 0..32 * 24 {
                buf.extend_from_slice(&[10, 120, 250]);
            }
            let mut jpg = Vec::new();
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90);
            enc.write_image(&buf, 32, 24, image::ExtendedColorType::Rgb8).unwrap();
            std::fs::write(out, jpg).unwrap();
            Ok(())
        }
    }

    /// A fake extractor that always fails: the corrupt/unsupported path.
    struct FakeFail;
    impl FrameExtractor for FakeFail {
        fn extract_frame(&self, _v: &Path, _o: &Path) -> Result<(), fotobase_video::VideoError> {
            Err(fotobase_video::VideoError::NoVideoStream)
        }
    }

    /// A slow fake extractor: sleeps before writing, so an ordering test can
    /// observe that the video job is still running.
    struct FakeSlow;
    impl FrameExtractor for FakeSlow {
        fn extract_frame(&self, _v: &Path, out: &Path) -> Result<(), fotobase_video::VideoError> {
            std::thread::sleep(std::time::Duration::from_millis(200));
            FakeOk.extract_frame(_v, out)
        }
    }

    fn catalog_with_media(dir: &TempDir, hash: &str, path: &str, media_type: &str) -> Catalog {
        let mut cat = Catalog::open(dir.path().join("catalog.db")).unwrap();
        let folder = cat.seed_folder("/photos").unwrap();
        cat.conn()
            .execute(
                "INSERT INTO files(folder_id, path, filename, size, mtime, hash, media_type)
                 VALUES (?1, ?2, ?3, 1, 0.0, ?4, ?5)",
                rusqlite::params![folder, path, "unused", Some(hash), media_type],
            )
            .unwrap();
        cat
    }

    fn tiny_video(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join("clip.mp4");
        std::fs::write(&p, b"fake mp4 bytes").unwrap();
        p
    }

    #[tokio::test]
    async fn video_miss_enqueues_a_background_priority_job_that_fills_the_cache() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("thumbs");
        let video = tiny_video(&tmp.path().join("photos"));
        let cat = catalog_with_media(
            &tmp,
            "cccccccccccccccccccccccccccccccc",
            &video.to_string_lossy(),
            "video",
        );
        let queue: Queue<ThumbOutcome> = Queue::new(8);

        let handles = enqueue_missing_thumbs(
            &req1("cccccccccccccccccccccccccccccccc"),
            &cat,
            &cache,
            &queue,
            None,
            Some(Arc::new(FakeOk)),
        );
        assert_eq!(handles.len(), 1, "video miss → one video thumbnail job");
        for handle in handles {
            match &*handle.result().await {
                Outcome::Done(Ok(_)) => {}
                other => panic!("video thumbnail job must succeed, got {other:?}"),
            }
        }
        // The video thumbnail landed under the shared key scheme, served by
        // the same thumb:// resolution — a hit, not the placeholder.
        let reply = resolve_thumb(&req1(&cache_key(&video)), &cache);
        assert_eq!(reply.content_type, "image/jpeg", "video thumbnail cached via thumb://");
        queue.close().await;
    }

    #[tokio::test]
    async fn video_jobs_run_after_visible_image_jobs() {
        // Ordering criterion: a video path's thumbnail job must dequeue
        // *after* a visible image job (PLAN §3 rule 4). A gate job holds the
        // single worker; video (background) is enqueued before the image
        // (visible) yet must finish after it.
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("thumbs");
        let video = tiny_video(&tmp.path().join("photos"));
        let img = gen_jpeg(&tmp.path().join("photos"), 9, 100, 80);

        let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
        let folder = cat.seed_folder("/photos").unwrap();
        for (hash, path, mt) in [
            ("dddddddddddddddddddddddddddddddd", video.to_string_lossy().to_string(), "video"),
            ("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", img.to_string_lossy().to_string(), "image"),
        ] {
            cat.conn()
                .execute(
                    "INSERT INTO files(folder_id, path, filename, size, mtime, hash, media_type)
                     VALUES (?1, ?2, ?3, 1, 0.0, ?4, ?5)",
                    rusqlite::params![folder, path, "unused", Some(hash), mt],
                )
                .unwrap();
        }

        let queue: Queue<ThumbOutcome> = Queue::new(8);
        use fotobase_jobs::{job, Outcome};
        let release = Arc::new(tokio::sync::Notify::new());
        let wait = release.clone();
        let gate = job(move |_ctx| {
            let wait = wait.clone();
            async move {
                wait.notified().await;
                Outcome::Done(Ok(fotobase_render::Thumbnail {
                    path: PathBuf::new(),
                    width: 0,
                    height: 0,
                    from_cache: false,
                }))
            }
        });
        let gate_handle = queue.try_enqueue("gate", Priority::Visible, gate).unwrap();

        let video_jobs = enqueue_missing_thumbs(
            &req1("dddddddddddddddddddddddddddddddd"),
            &cat,
            &cache,
            &queue,
            None,
            Some(Arc::new(FakeSlow)),
        );
        let image_jobs = enqueue_missing_thumbs(
            &req1("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            &cat,
            &cache,
            &queue,
            None,
            None,
        );
        assert_eq!(video_jobs.len(), 1);
        assert_eq!(image_jobs.len(), 1);

        release.notify_one();
        let _ = gate_handle.result().await;

        // The image job completes while the video job is still running: the
        // slow fake extractor gives the video job a 200 ms window, so right
        // after the image result lands the video cache entry cannot exist
        // yet — the video job was dequeued after the visible image job.
        match &*image_jobs[0].result().await {
            Outcome::Done(Ok(_)) => {}
            other => panic!("image job must succeed, got {other:?}"),
        }
        assert!(
            !cache.join(format!("{}.jpg", cache_key(&video))).exists(),
            "video job must not have completed before the visible image job"
        );
        match &*video_jobs[0].result().await {
            Outcome::Done(Ok(_)) => {}
            other => panic!("video job must succeed, got {other:?}"),
        }
        assert!(cache.join(format!("{}.jpg", cache_key(&video))).exists());
        queue.close().await;
    }

    #[tokio::test]
    async fn corrupt_video_yields_error_result_and_placeholder_not_crash() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("thumbs");
        let video = tiny_video(&tmp.path().join("photos"));
        let cat = catalog_with_media(
            &tmp,
            "ffffffffffffffffffffffffffffffff",
            &video.to_string_lossy(),
            "video",
        );
        let queue: Queue<ThumbOutcome> = Queue::new(8);

        let handles = enqueue_missing_thumbs(
            &req1("ffffffffffffffffffffffffffffffff"),
            &cat,
            &cache,
            &queue,
            None,
            Some(Arc::new(FakeFail)),
        );
        assert_eq!(handles.len(), 1);
        for handle in handles {
            match &*handle.result().await {
                Outcome::Done(Err(_)) => {}
                other => panic!("corrupt video must be a job-level Err, got {other:?}"),
            }
        }
        // The scheme layer serves the placeholder — never an error dialog.
        let reply = resolve_thumb(&req1("ffffffffffffffffffffffffffffffff"), &cache);
        assert!(reply.body.starts_with(&[0x89, b'P', b'N', b'G']), "placeholder served");
        queue.close().await;
    }
}
