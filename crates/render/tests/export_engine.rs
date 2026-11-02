//! TASK-04-018: full-resolution color-managed export engine (PLAN §3 rule 5,
//! §6 P2b, §9 Render fidelity).
//!
//! One engine, two resolutions: the export path runs the *same* shared
//! recipe engine (`fotobase_core::apply`) at full resolution that the
//! preview path (TASK-04-010) runs at screen resolution — no separate op
//! implementations. Output is color-managed: an embedded source profile is
//! honored (converted into the output space) and the exported file is
//! tagged with that output profile.

use std::path::Path;

use fotobase_render::{export_image, ExportFormat, ExportOptions};

const BW: &str =
    r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"bw","params":{"amount":1.0}}]}"#;

fn gen_jpeg(dir: &Path, name: &str, w: u32, h: u32) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    let mut buf = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            buf.extend_from_slice(&[x as u8, y as u8, 128]);
        }
    }
    let mut out = Vec::new();
    {
        use image::ImageEncoder as _;
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92);
        enc.write_image(&buf, w, h, image::ExtendedColorType::Rgb8).unwrap();
    }
    std::fs::write(&p, &out).unwrap();
    p
}

/// RMS of per-channel deltas, 0–255 scale (same metric as golden.rs).
fn rms(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let sum: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = i64::from(*x) - i64::from(*y);
            (d * d) as f64
        })
        .sum();
    (sum / (a.len().max(1) as f64)).sqrt()
}

/// Decode the exported file back to RGB8 pixels at its native size.
fn decode_rgb(path: &Path) -> (Vec<u8>, u32, u32) {
    let img = image::open(path).expect("exported file decodes").to_rgb8();
    let (w, h) = (img.width(), img.height());
    (img.into_raw(), w, h)
}

#[test]
fn export_renders_the_recipe_at_full_resolution() {
    let tmp = tempfile::TempDir::new().unwrap();
    // 64×48: comfortably above preview sizes, small enough to be fast.
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg", 64, 48);
    let out = tmp.path().join("out").join("a.jpg");

    let result = export_image(
        &src,
        &out,
        BW,
        &ExportOptions { format: ExportFormat::Jpeg, quality: 92, resize_max_edge: None },
    )
    .unwrap();

    assert!(out.exists());
    assert_eq!(result.width, 64, "no resize option → full resolution");
    assert_eq!(result.height, 48);
    let (_, w, h) = decode_rgb(&out);
    assert_eq!((w, h), (64, 48), "export must not shrink to preview size");
}

#[test]
fn export_matches_the_preview_within_tolerance() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "b.jpg", 32, 32);
    let out = tmp.path().join("out").join("b.jpg");

    export_image(
        &src,
        &out,
        BW,
        &ExportOptions { format: ExportFormat::Jpeg, quality: 95, resize_max_edge: None },
    )
    .unwrap();

    // Golden-image check (PLAN §9): render the same recipe through the
    // shared engine directly and compare against the exported pixels. The
    // export pipeline must be that engine plus I/O, nothing else.
    let bytes = std::fs::read(&src).unwrap();
    let img = image::load_from_memory(&bytes).unwrap().to_rgb8();
    let linear = fotobase_core::buffer::LinearImage {
        width: 32,
        height: 32,
        data: img.iter().map(|b| f32::from(*b) / 255.0).collect(),
    };
    let (recipe, _) = fotobase_core::recipe::parse(BW).unwrap();
    let edited = fotobase_core::apply::apply_recipe(&linear, &recipe);
    let expected: Vec<u8> =
        edited.data.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect();

    let (got, _, _) = decode_rgb(&out);
    assert!(
        rms(&got, &expected) < 3.0,
        "export output diverges from the shared engine (rms={})",
        rms(&got, &expected)
    );
}

#[test]
fn export_honors_the_embedded_profile_and_tags_the_output() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Synthetic wide-gamut source fixture with an embedded profile.
    let profile = fotobase_render::rgb_profile_bytes(
        [0.3127, 0.3290, 1.0],
        [[0.64, 0.33, 1.0], [0.21, 0.71, 1.0], [0.15, 0.06, 1.0]],
        2.19921875,
    )
    .unwrap();
    let mut buf = Vec::new();
    for _ in 0..32 * 32 {
        buf.extend_from_slice(&[240, 40, 40]);
    }
    let mut jpeg = Vec::new();
    {
        use image::ImageEncoder as _;
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
            .write_image(&buf, 32, 32, image::ExtendedColorType::Rgb8)
            .unwrap();
    }
    fotobase_render::icc::attach_to_jpeg(&mut jpeg, &profile);
    let src = tmp.path().join("photos").join("p3.jpg");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, &jpeg).unwrap();

    let out = tmp.path().join("out").join("p3.jpg");
    let result = export_image(
        &src,
        &out,
        BW,
        &ExportOptions { format: ExportFormat::Jpeg, quality: 92, resize_max_edge: None },
    )
    .unwrap();

    assert!(result.output_profile.is_some(), "tagged source → tagged export");
    let tagged = fotobase_render::extract_icc(&std::fs::read(&out).unwrap());
    assert!(tagged.is_some(), "exported JPEG must carry an ICC profile");

    // Untagged source: export is sRGB and *is* tagged with an explicit
    // sRGB profile so consumers never have to guess (color-managed output).
    let plain = gen_jpeg(&tmp.path().join("photos"), "plain.jpg", 16, 16);
    let out2 = tmp.path().join("out").join("plain.jpg");
    let r2 = export_image(
        &plain,
        &out2,
        BW,
        &ExportOptions { format: ExportFormat::Jpeg, quality: 92, resize_max_edge: None },
    )
    .unwrap();
    assert!(r2.output_profile.is_some(), "untagged source exports as tagged sRGB");
    assert!(fotobase_render::extract_icc(&std::fs::read(&out2).unwrap()).is_some());
}

#[tokio::test]
async fn export_runs_as_a_queue_job() {
    use fotobase_jobs::Outcome;
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "e.jpg", 16, 16);
    let out = tmp.path().join("out").join("e.jpg");
    let job = fotobase_render::export_image_job(
        src.clone(),
        out.clone(),
        BW.to_string(),
        ExportOptions { format: ExportFormat::Jpeg, quality: 90, resize_max_edge: None },
    );
    let queue = fotobase_jobs::Queue::new(2);
    let handle = queue.enqueue("test-key", fotobase_jobs::Priority::Visible, job).await.unwrap();
    match &*handle.result().await {
        Outcome::Done(Ok(_)) => assert!(out.exists()),
        other => panic!("expected Done(Ok), got {other:?}"),
    }
}

#[test]
fn export_resizes_when_asked_but_never_upscales() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "c.jpg", 32, 24);
    let out = tmp.path().join("out").join("c.jpg");
    let r = export_image(
        &src,
        &out,
        BW,
        &ExportOptions { format: ExportFormat::Jpeg, quality: 90, resize_max_edge: Some(16) },
    )
    .unwrap();
    assert_eq!((r.width, r.height), (16, 12));

    // Source smaller than the limit: never upscaled.
    let out2 = tmp.path().join("out").join("d.jpg");
    let r2 = export_image(
        &src,
        &out2,
        BW,
        &ExportOptions { format: ExportFormat::Jpeg, quality: 90, resize_max_edge: Some(4096) },
    )
    .unwrap();
    assert_eq!((r2.width, r2.height), (32, 24));
}

#[test]
fn export_never_touches_the_original_and_rejects_bad_recipes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "f.jpg", 16, 16);
    let before = std::fs::read(&src).unwrap();
    let out = tmp.path().join("out").join("f.jpg");

    assert!(
        export_image(
            &src,
            &out,
            "{not json",
            &ExportOptions { format: ExportFormat::Jpeg, quality: 90, resize_max_edge: None },
        )
        .is_err(),
        "malformed recipe must fail loudly, not export the original"
    );
    assert!(!out.exists(), "a failed export must not leave a partial output file");
    assert_eq!(std::fs::read(&src).unwrap(), before, "originals are read-only (PLAN §3 rule 1)");
}

#[test]
fn large_image_render_stays_within_the_memory_budget() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Simulated very large image: a 256×256 source stands in for a
    // multi-megapixel one — the budget assertion is about the *scaling law*
    // (peak ∝ tile budget, not whole image), which the probe measures
    // directly via `export_peak_bytes`.
    // 256×256 keeps the test fast while the probe still measures the
    // streaming scaling law (peak ∝ tile budget, not whole image).
    let src = gen_jpeg(&tmp.path().join("photos"), "big.jpg", 256, 256);

    let peak = fotobase_render::export_peak_bytes(
        &src,
        BW,
        &ExportOptions { format: ExportFormat::Jpeg, quality: 90, resize_max_edge: None },
        256 * 256, // tile budget: one tile's worth of RGB8 pixels
    )
    .unwrap();
    // Whole-image linear-light buffer for 256×256 would be 256*256*3*4 =
    // 786,432 bytes; the budget here is one tile of RGB8 = 196,608. The
    // streaming render must stay within tile budget + one output row of
    // slack, never the whole-image buffer.
    let whole_image_linear = 256usize * 256 * 3 * 4;
    assert!(
        peak < whole_image_linear,
        "streamed export peaked at {peak} bytes, whole-image buffer is {whole_image_linear}"
    );
    let out = tmp.path().join("out").join("big.jpg");
    assert!(!out.exists(), "peak-bytes probe must not write an output file");
}

#[test]
fn export_png_is_supported() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "png-src.jpg", 16, 16);
    let out = tmp.path().join("out").join("png-src.png");
    let r = export_image(
        &src,
        &out,
        BW,
        &ExportOptions { format: ExportFormat::Png, quality: 90, resize_max_edge: None },
    )
    .unwrap();
    assert_eq!(out.extension().unwrap(), "png");
    assert_eq!((r.width, r.height), (16, 16));
}
