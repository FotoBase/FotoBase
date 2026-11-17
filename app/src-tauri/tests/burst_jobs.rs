//! TASK-07-006 integration: the quality-scoring pass as a job-queue job over
//! real decoded images. One blurred and one sharp frame — the sharp frame
//! scores higher and, once grouped, would be the suggested best. Engine
//! fakes keep the pass testable; the real scorer is the ai crate's own
//! coverage.

use std::sync::atomic::AtomicBool;

use fotobase_catalog::Catalog;
use fotobase_lib::burst_jobs::{run_quality_pass, QualityEngine};
use fotobase_scanner::scan_folder;

/// A sharp checkerboard JPEG (high local contrast).
fn sharp_jpeg() -> Vec<u8> {
    let mut img = image::RgbImage::new(64, 48);
    for (x, _, p) in img.enumerate_pixels_mut() {
        let dark = (x / 4) % 2 == 0;
        *p = if dark { image::Rgb([250, 250, 250]) } else { image::Rgb([5, 5, 5]) };
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .unwrap();
    out
}

/// The same pattern box-blurred to a 4px-wide average: an out-of-focus frame.
fn blurry_jpeg() -> Vec<u8> {
    let mut img = image::RgbImage::new(64, 48);
    for (x, _, p) in img.enumerate_pixels_mut() {
        let band = x / 8; // 8px bands = a soft gradient after JPEG encode
        *p = if band % 2 == 0 { image::Rgb([240, 240, 240]) } else { image::Rgb([15, 15, 15]) };
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .unwrap();
    out
}

/// Deterministic fake: sharpness derived from the red channel's local
/// contrast, so the checkerboard outranks the soft gradient. Mirrors what
/// the real Laplacian scorer sees.
struct FakeEngine;

impl QualityEngine for FakeEngine {
    fn score(&mut self, rgb: &[u8], w: u32, h: u32) -> Result<f64, String> {
        let mut contrast = 0u64;
        for y in 0..h as usize {
            for x in 1..w as usize {
                contrast += (rgb[(y * w as usize + x) * 3] as i64
                    - rgb[(y * w as usize + x - 1) * 3] as i64)
                    .unsigned_abs();
            }
        }
        Ok(contrast as f64)
    }
}

fn fixture() -> (tempfile::TempDir, Catalog, i64, i64) {
    let lib = tempfile::tempdir().unwrap();
    std::fs::write(lib.path().join("sharp.jpg"), sharp_jpeg()).unwrap();
    std::fs::write(lib.path().join("blurry.jpg"), blurry_jpeg()).unwrap();
    let db = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(db.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, lib.path(), &AtomicBool::new(false)).unwrap();
    let ids: Vec<(i64, String)> = cat
        .conn()
        .prepare("SELECT id, filename FROM files ORDER BY filename")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(ids.len(), 2);
    let blurry = ids.iter().find(|(_, n)| n == "blurry.jpg").unwrap().0;
    let sharp = ids.iter().find(|(_, n)| n == "sharp.jpg").unwrap().0;
    (lib, cat, blurry, sharp)
}

#[test]
fn the_sharp_frame_scores_above_the_blurred_one() {
    let (_lib, mut cat, blurry, sharp) = fixture();
    let report = run_quality_pass(&mut cat, &mut FakeEngine, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.scored, 2);
    assert_eq!(report.failed, 0);
    let blurry_score = cat.quality_score(blurry).unwrap().unwrap();
    let sharp_score = cat.quality_score(sharp).unwrap().unwrap();
    assert!(sharp_score > blurry_score, "sharp {sharp_score} vs blurry {blurry_score}");
}

#[test]
fn an_undecodable_frame_is_marked_failed_not_fatal() {
    let (_lib, mut cat, _blurry, _sharp) = fixture();
    std::fs::write(
        cat.conn()
            .query_row("SELECT path FROM files WHERE filename = 'sharp.jpg'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        b"not an image",
    )
    .unwrap();
    let report = run_quality_pass(&mut cat, &mut FakeEngine, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.scored, 1);
    assert_eq!(report.failed, 1);
}

#[test]
fn a_cancelled_pass_stops_early_with_valid_state() {
    let (_lib, mut cat, _blurry, _sharp) = fixture();
    let flag = AtomicBool::new(true);
    let report = run_quality_pass(&mut cat, &mut FakeEngine, &flag).unwrap();
    assert_eq!(report.scored, 0);
}
