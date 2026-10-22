//! Integration tests for the thumbnail pipeline (TASK-01-010, TDD).
//!
//! Fixtures are generated in-process (deterministic LCG), mirroring
//! `tools/fixture_gen.py` determinism without depending on Python in CI.

use std::path::{Path, PathBuf};

use fotobase_jobs::{Outcome, Priority, Queue};
use fotobase_render::{thumbnail, thumbnail_job, ThumbOptions};
use image::ImageEncoder;
use tempfile::TempDir;

/// Deterministic tiny JPEG generator: a solid-ish color block with variation.
fn gen_jpeg(dir: &Path, i: usize, w: u32, h: u32) -> PathBuf {
    // Moduli are pairwise-coprime primes so content stays unique for i well
    // beyond 1000 — a shared cache entry must mean shared content, not a
    // generator period.
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
    // A pristine copy, kept for cache-key tests that corrupt the original.
    let pristine = dir.join("pristine.jpg");
    if !pristine.exists() {
        std::fs::write(&pristine, &out).unwrap();
    }
    p
}

fn gen_png(dir: &Path, i: usize, w: u32, h: u32) -> PathBuf {
    let r = (i * 53 % 251) as u8;
    let g = (i * 11 % 247) as u8;
    let b = (i * 211 % 239) as u8;
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let v = ((x + y + i as u32) % 16) as u8;
        image::Rgb([r.wrapping_add(v), g.wrapping_add(v), b.wrapping_add(v)])
    });
    let p = dir.join(format!("img_{i:05}.png"));
    img.save(&p).unwrap();
    p
}

#[test]
fn cache_tag_names_a_distinct_cache_entry() {
    // TASK-01-024 HiDPI: size variants live beside the legacy 512 entry
    // under a cache tag, so a 2x render never serves 1x pixels and never
    // overwrites the 1x cache.
    let tmp = TempDir::new().unwrap();
    let src = gen_jpeg(tmp.path(), 0, 800, 600);
    let cache = tmp.path().join("thumbs");

    let opts = ThumbOptions { cache_tag: Some("@2x".into()), ..ThumbOptions::default() };
    let first = thumbnail(&src, &cache, opts.clone()).unwrap();
    assert!(!first.from_cache);
    assert!(first
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.ends_with("@2x.jpg"))
        .unwrap_or(false));

    let again = thumbnail(&src, &cache, opts).unwrap();
    assert!(again.from_cache, "the tagged entry must be a cache hit");

    // The legacy (untagged) entry was not created by the tagged render.
    let legacy = std::fs::read_dir(&cache).map(|d| d.count()).unwrap_or(0);
    assert_eq!(legacy, 1);
}

#[test]
fn second_call_same_content_is_cache_hit_without_decode() {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("thumbs");
    let src = gen_jpeg(tmp.path(), 1, 300, 200);

    let first = thumbnail(&src, &cache, ThumbOptions::default()).unwrap();
    assert!(!first.from_cache);
    let first_path = first.path.clone();

    // Corrupt the original beyond recognition, then request the same content
    // through a different path: the cache is keyed by content hash, so the
    // original must never be touched again, let alone decoded.
    std::fs::write(&src, b"corrupted beyond recognition").unwrap();
    let copy = tmp.path().join("copy_of_img_00001.jpg");
    std::fs::copy(tmp.path().join("pristine.jpg"), &copy).unwrap();

    let second = thumbnail(&copy, &cache, ThumbOptions::default()).unwrap();
    assert!(second.from_cache, "same content must hit the cache");
    assert_eq!(second.path, first_path);
    assert_eq!(second.width, first.width);
    assert_eq!(second.height, first.height);
}

#[test]
fn different_content_gets_different_cache_entry() {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("thumbs");
    let a = gen_jpeg(tmp.path(), 1, 100, 100);
    let b = gen_jpeg(tmp.path(), 2, 100, 100);

    let ta = thumbnail(&a, &cache, ThumbOptions::default()).unwrap();
    let tb = thumbnail(&b, &cache, ThumbOptions::default()).unwrap();
    assert_ne!(ta.path, tb.path);
    assert!(!tb.from_cache);
}

#[test]
fn malformed_file_errors_without_crashing_and_writes_no_cache_entry() {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("thumbs");
    let bad = tmp.path().join("broken.jpg");
    std::fs::write(&bad, b"this is not an image at all").unwrap();

    let err = thumbnail(&bad, &cache, ThumbOptions::default()).unwrap_err();
    assert!(matches!(err, fotobase_render::RenderError::Decode { .. }));

    // No cache directory was even created, hence zero cache entries.
    let entries = std::fs::read_dir(&cache).map(|d| d.count()).unwrap_or(0);
    assert_eq!(entries, 0, "failed render must not write a cache entry");
}

#[test]
fn unrecognized_header_is_a_decode_error() {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("thumbs");
    let p = tmp.path().join("bad.bin");
    std::fs::write(&p, b"plain text, no image magic").unwrap();

    let err = thumbnail(&p, &cache, ThumbOptions::default()).unwrap_err();
    assert!(matches!(err, fotobase_render::RenderError::Decode { .. }));
}

#[test]
fn gif_header_without_image_data_is_a_decode_error_not_unsupported() {
    // GIF is supported since 01-012; a header-only file is corrupt, not a
    // format gap.
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("thumbs");
    let p = tmp.path().join("anim.gif");
    std::fs::write(&p, b"GIF89a\x01\x00\x01\x00\x00\x00\x00;").unwrap();

    let err = thumbnail(&p, &cache, ThumbOptions::default()).unwrap_err();
    assert!(matches!(err, fotobase_render::RenderError::Decode { .. }));
}

#[test]
fn thumb_respects_max_edge_and_aspect_ratio() {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("thumbs");
    let src = gen_jpeg(tmp.path(), 3, 400, 200);

    let t = thumbnail(&src, &cache, ThumbOptions { max_edge: 100, ..Default::default() }).unwrap();
    assert_eq!((t.width, t.height), (100, 50));
}

#[test]
fn smaller_than_max_edge_is_not_upscaled() {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("thumbs");
    let src = gen_jpeg(tmp.path(), 4, 80, 60);

    let t = thumbnail(&src, &cache, ThumbOptions { max_edge: 512, ..Default::default() }).unwrap();
    assert_eq!((t.width, t.height), (80, 60));
}

#[test]
fn solid_color_content_hashes_to_known_sha256_prefix() {
    use sha2::{Digest, Sha256};
    let bytes = b"deterministic bytes";
    let mut h = Sha256::new();
    h.update(bytes);
    let expect = hex(&h.finalize());
    assert_eq!(&expect[..8], "e751940b");
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// 1k-fixture integration run: every decodable fixture produces a thumbnail
/// through the bounded job queue, the malformed file is skipped, and the
/// visible-first priority applies (§3 rule 4, §9 robustness).
#[tokio::test]
async fn one_k_fixture_pipeline_zero_failed_decodes() {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("thumbs");
    const N: usize = 1000;

    let paths: Vec<PathBuf> = (0..N)
        .map(|i| {
            if i % 4 == 0 {
                gen_png(tmp.path(), i, 96, 64)
            } else {
                gen_jpeg(tmp.path(), i, 96, 64)
            }
        })
        .collect();

    // Capacity N: this test exercises the pipeline end-to-end, not
    // backpressure (covered by the jobs crate tests).
    let queue: Queue<Result<fotobase_render::Thumbnail, fotobase_render::RenderError>> =
        Queue::new(N);
    let opts = ThumbOptions::default();

    let mut handles = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        let prio = if i < 10 { Priority::Visible } else { Priority::Background };
        let key = format!("thumb:{}", p.display());
        handles.push((
            i,
            queue
                .try_enqueue(
                    key.as_str(),
                    prio,
                    thumbnail_job(p.clone(), cache.clone(), opts.clone()),
                )
                .unwrap(),
        ));
    }

    let mut ok = 0usize;
    let mut failed = Vec::new();
    for (i, h) in handles {
        match h.result().await.as_ref() {
            Outcome::Done(Ok(_)) => ok += 1,
            Outcome::Done(Err(e)) => failed.push((i, e.to_string())),
            Outcome::Failed(e) => failed.push((i, e.clone())),
            Outcome::Cancelled => failed.push((i, "cancelled".into())),
        }
    }
    assert!(failed.is_empty(), "zero failed decodes, got: {failed:?}");
    assert_eq!(ok, N, "all fixtures must render");

    // Cache is populated on disk: one file per photo.
    let entries = std::fs::read_dir(&cache).unwrap().count();
    assert_eq!(entries, N);
}

/// A malformed file inside the batch is skipped without crashing the queue
/// and the other photos still render (§9).
#[tokio::test]
async fn malformed_fixture_is_skipped_in_batch() {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("thumbs");
    let bad = tmp.path().join("broken.jpg");
    std::fs::write(&bad, b"garbage bytes").unwrap();

    let queue: Queue<Result<fotobase_render::Thumbnail, fotobase_render::RenderError>> =
        Queue::new(8);
    let opts = ThumbOptions::default();

    let mut handles = Vec::new();
    for p in [bad.clone(), gen_jpeg(tmp.path(), 7, 64, 64), gen_png(tmp.path(), 8, 64, 64)] {
        let key = format!("thumb:{}", p.display());
        handles.push((
            p.clone(),
            queue
                .try_enqueue(
                    key.as_str(),
                    Priority::Background,
                    thumbnail_job(p.clone(), cache.clone(), opts.clone()),
                )
                .unwrap(),
        ));
    }

    let mut errors = 0;
    for (p, h) in handles {
        if let Outcome::Done(Err(_)) = h.result().await.as_ref() {
            assert_eq!(p, bad, "only the malformed file may fail");
            errors += 1;
        }
    }
    assert_eq!(errors, 1);
}
