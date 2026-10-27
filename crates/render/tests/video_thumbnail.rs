//! Video thumbnails through the shared cache pipeline (TDD, task 02-004).
//!
//! The video's cache key is the content hash of the *video file* — the same
//! key the catalog stores and `thumb://` serves — not the hash of the
//! extracted frame. Extraction failures (corrupt/unsupported media) are
//! `Err`s, never panics (PLAN §9); the scheme layer turns those into the
//! placeholder.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fotobase_jobs::Outcome;
use fotobase_render::{video_thumbnail, video_thumbnail_job, RenderError, ThumbOptions};
use fotobase_video::FrameExtractor;
use image::ImageEncoder as _;

/// A tiny valid JPEG (red 64×48), re-encoded per call so each temp file is
/// independently decodable.
fn jpeg_bytes() -> Vec<u8> {
    let mut buf = Vec::new();
    for _ in 0..64 * 48 {
        buf.extend_from_slice(&[200, 30, 30]);
    }
    let mut out = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
    enc.write_image(&buf, 64, 48, image::ExtendedColorType::Rgb8).unwrap();
    out
}

/// Fake extractor: pretends ffmpeg ran and writes a decodable JPEG frame.
struct FakeOk;

impl FrameExtractor for FakeOk {
    fn extract_frame(&self, _video: &Path, out: &Path) -> Result<(), fotobase_video::VideoError> {
        std::fs::write(out, jpeg_bytes()).unwrap();
        Ok(())
    }
}

/// Fake extractor that always fails — the corrupt/unsupported-file path.
struct FakeFail;

impl FrameExtractor for FakeFail {
    fn extract_frame(&self, _video: &Path, _out: &Path) -> Result<(), fotobase_video::VideoError> {
        Err(fotobase_video::VideoError::NoVideoStream)
    }
}

/// The content hash the catalog stores / `thumb://` serves: first 16 bytes
/// of SHA-256, lowercase hex, streamed (videos can be gigabytes).
fn content_hash(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).unwrap();
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).unwrap();
    hasher.finalize()[..16].iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write as _;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

fn video_file(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join("clip.mp4");
    // Placeholder bytes: the fake extractor never reads them, but the hash
    // must be computed from the real video file.
    std::fs::write(&p, b"fake mp4 container bytes").unwrap();
    p
}

#[test]
fn video_thumbnail_caches_under_the_video_content_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("thumbs");
    let video = video_file(&tmp.path().join("photos"));

    let thumb = video_thumbnail(&video, &cache, ThumbOptions::default(), &FakeOk).unwrap();
    let key = content_hash(&video);
    let name = thumb.path.file_name().unwrap().to_str().unwrap().to_string();
    assert_eq!(name, format!("{key}.jpg"), "same cache key as images");
    assert!(!thumb.from_cache);
    assert_eq!((thumb.width, thumb.height), (64, 48), "no upscale beyond source");

    // Second call is a cache hit — ffmpeg does not run again.
    let again = video_thumbnail(&video, &cache, ThumbOptions::default(), &FakeFail).unwrap();
    assert!(again.from_cache, "cache hit must not re-extract");
    assert_eq!(again.path, thumb.path);
}

#[test]
fn video_thumbnail_uses_the_2x_cache_tag() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("thumbs");
    let video = video_file(&tmp.path().join("photos"));
    let opts = ThumbOptions { max_edge: 1024, cache_tag: Some("@2x".into()), ..Default::default() };

    let thumb = video_thumbnail(&video, &cache, opts, &FakeOk).unwrap();
    assert!(thumb.path.file_name().unwrap().to_str().unwrap().ends_with("@2x.jpg"));
}

#[test]
fn failed_extraction_is_an_error_not_a_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let video = video_file(&tmp.path().join("photos"));
    let result =
        video_thumbnail(&video, &tmp.path().join("thumbs"), ThumbOptions::default(), &FakeFail);
    assert!(matches!(result, Err(RenderError::Decode { .. })));
    // And nothing was cached: the next request retries (or serves the
    // placeholder), a corrupt file never poisons the cache.
    let entries = std::fs::read_dir(tmp.path().join("thumbs")).map(|d| d.count()).unwrap_or(0);
    assert_eq!(entries, 0, "no cache entry for a failed extraction");
}

#[tokio::test]
async fn video_thumbnail_job_runs_in_the_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("thumbs");
    let video = video_file(&tmp.path().join("photos"));
    let extractor: Arc<dyn FrameExtractor + Send + Sync> = Arc::new(FakeOk);

    let q = fotobase_jobs::Queue::new(4);
    let handle = q
        .try_enqueue(
            "vid",
            fotobase_jobs::Priority::Background,
            video_thumbnail_job(video.clone(), cache.clone(), ThumbOptions::default(), extractor),
        )
        .unwrap();
    match &*handle.result().await {
        Outcome::Done(Ok(_)) => {}
        other => panic!("video thumbnail job must succeed, got {other:?}"),
    }
    assert!(cache.join(format!("{}.jpg", content_hash(&video))).exists());
    q.close().await;
}
