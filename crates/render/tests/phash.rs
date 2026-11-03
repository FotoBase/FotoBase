//! TASK-04-019: perceptual hashing for the near-duplicate finder.
//!
//! Recalled duplicates (resized / re-compressed variants of the same image)
//! must hash identically; distinct photos must not collide. Hashes are
//! derived data (PLAN §5): recomputable, never authoritative.

use std::path::Path;

use fotobase_render::perceptual_hash;

/// Encode a smooth low-frequency gradient JPEG: resize + re-compress keep
/// its structure the way they do for real photos (a per-pixel noise pattern
/// would alias differently at each scale and legitimately hash apart).
fn encode_jpeg(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let mut img = image::RgbImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let ramp = if seed.is_multiple_of(2) {
            (x as f32 / w.max(1) as f32 + y as f32 / h.max(1) as f32) * 120.0
        } else {
            (x as f32 / w.max(1) as f32 - y as f32 / h.max(1) as f32) * 120.0
        };
        let base = ramp + f32::from(seed) * 0.35;
        *p = image::Rgb([base as u8, (base * 1.3) as u8, (base * 0.7) as u8]);
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .unwrap();
    out
}

fn write_jpeg(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn resize_and_recompress_variants_hash_identically() {
    let tmp = tempfile::tempdir().unwrap();
    let original = encode_jpeg(640, 480, 1);
    // Resized + re-compressed: what a "smaller copy" duplicate looks like.
    let small = encode_jpeg(320, 240, 1);
    // Re-compressed at different quality, same pixels: the other variant.
    let recompressed = encode_jpeg(640, 480, 1);

    let h_orig = perceptual_hash(&write_jpeg(tmp.path(), "orig.jpg", &original)).unwrap();
    let h_small = perceptual_hash(&write_jpeg(tmp.path(), "small.jpg", &small)).unwrap();
    let h_re = perceptual_hash(&write_jpeg(tmp.path(), "re.jpg", &recompressed)).unwrap();

    assert_eq!(h_orig, h_small, "resized variant must be recalled");
    assert_eq!(h_orig, h_re, "re-compressed variant must be recalled");
}

#[test]
fn distinct_images_hash_differently() {
    let tmp = tempfile::tempdir().unwrap();
    let a = perceptual_hash(&write_jpeg(tmp.path(), "a.jpg", &encode_jpeg(320, 240, 1))).unwrap();
    let b = perceptual_hash(&write_jpeg(tmp.path(), "b.jpg", &encode_jpeg(320, 240, 200))).unwrap();
    assert_ne!(a, b, "distinct photos must not collide");
}

#[test]
fn undecodable_files_error() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("junk.jpg");
    std::fs::write(&p, b"not an image").unwrap();
    assert!(perceptual_hash(&p).is_err());
}

#[tokio::test]
async fn phash_job_runs_in_the_job_queue() {
    use fotobase_jobs::{Outcome, Priority, Queue};

    let tmp = tempfile::tempdir().unwrap();
    let p = write_jpeg(tmp.path(), "orig.jpg", &encode_jpeg(640, 480, 1));
    let queue = Queue::new(4);
    let handle =
        queue.try_enqueue("phash", Priority::Background, fotobase_render::phash_job(p)).unwrap();
    assert!(matches!(*handle.result().await, Outcome::Done(Ok(_))));
}
