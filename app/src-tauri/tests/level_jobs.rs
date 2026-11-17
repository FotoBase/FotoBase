//! TASK-07-007 integration: the auto-level horizon estimate over real
//! decoded images. A tilted-horizon JPEG yields a confident estimate within
//! tolerance; a flat gradient sky declines (`None`).

use fotobase_lib::level_jobs::{estimate_for_path, PureHorizonEngine};

/// A JPEG with a bright sky above a dark ground, horizon tilted by
/// `deg` degrees (clockwise positive), soft-edged via a small blur.
fn tilted_horizon_jpeg(deg: f64, size: u32) -> Vec<u8> {
    let slope = deg.to_radians().tan();
    let mut img = image::RgbImage::new(size, size);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let y_edge = size as f64 * 0.5 + slope * (x as f64 - size as f64 / 2.0);
        let v = if (y as f64) < y_edge { 220u8 } else { 50u8 };
        *p = image::Rgb([v, v, v]);
    }
    let blurred = image::imageops::blur(&img, 1.5);
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(blurred)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .unwrap();
    out
}

/// A JPEG with no edge at all: smooth vertical gradient (flat cloudy sky).
fn flat_sky_jpeg(size: u32) -> Vec<u8> {
    let mut img = image::RgbImage::new(size, size);
    for (_, y, p) in img.enumerate_pixels_mut() {
        let v = (90 + y * 60 / size) as u8;
        *p = image::Rgb([v, v, v]);
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .unwrap();
    out
}

fn write_temp(dir: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn a_tilted_horizon_yields_a_confident_leveling_angle() {
    let dir = tempfile::tempdir().unwrap();
    for deg in [-6.0f64, 5.0] {
        let path =
            write_temp(dir.path(), &format!("tilt{deg}.jpg"), &tilted_horizon_jpeg(deg, 512));
        let est = estimate_for_path(&path, &mut PureHorizonEngine)
            .expect("decode must succeed")
            .expect("a clear horizon must be found");
        assert!(
            (est.angle_deg as f64 + deg).abs() < 1.5,
            "leveling angle {est:?} must undo a {deg}° tilt"
        );
        assert!(est.confidence > 0.35, "confident estimate, got {:?}", est.confidence);
    }
}

#[test]
fn a_flat_sky_declines_instead_of_guessing() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_temp(dir.path(), "flat.jpg", &flat_sky_jpeg(512));
    let est = estimate_for_path(&path, &mut PureHorizonEngine).expect("decode must succeed");
    assert!(est.is_none(), "no horizon must not apply an op: {est:?}");
}

#[test]
fn an_undecodable_file_is_an_error_not_a_guess() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_temp(dir.path(), "junk.jpg", b"not an image");
    assert!(estimate_for_path(&path, &mut PureHorizonEngine).is_err());
}
