//! TASK-04-010: recipe preview rendering (PLAN §2D live preview, §3 rules
//! 3–5). The edit view's preview is the original decoded, the recipe folded
//! over it by the shared engine, resized to screen resolution and cached on
//! disk — the same apply path the golden tests pin and export reuses.

use std::path::Path;

use fotobase_core::recipe;
use fotobase_render::recipe_preview;

fn gen_jpeg(dir: &Path, name: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    let mut buf = Vec::with_capacity(16 * 12 * 3);
    for y in 0..12u32 {
        for x in 0..16u32 {
            buf.extend_from_slice(&[x as u8 * 16, y as u8 * 20, 128]);
        }
    }
    let mut out = Vec::new();
    {
        use image::ImageEncoder as _;
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92);
        enc.write_image(&buf, 16, 12, image::ExtendedColorType::Rgb8).unwrap();
    }
    std::fs::write(&p, &out).unwrap();
    p
}

const BW: &str =
    r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"bw","params":{"amount":1.0}}]}"#;

#[test]
fn recipe_preview_renders_a_recipe_change_into_a_cache_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let cache = tmp.path().join("cache");

    let first = recipe_preview(&src, &cache, BW, 512, 85).unwrap();
    assert!(!first.from_cache);
    assert!(first.path.exists());
    assert!(first.path.is_absolute() || first.path.starts_with(&cache));
    assert_eq!(first.path.extension().unwrap(), "jpg");
    assert!(
        first.path.to_string_lossy().contains("edit"),
        "edit previews live in their own cache subtree"
    );

    // Same recipe again: cache hit, byte-identical file.
    let second = recipe_preview(&src, &cache, BW, 512, 85).unwrap();
    assert!(second.from_cache);
    assert_eq!(second.path, first.path);

    // A different recipe must not reuse the first one's pixels.
    let warm = r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"temperature","params":{"kelvin_shift":0.5}}]}"#;
    let third = recipe_preview(&src, &cache, warm, 512, 85).unwrap();
    assert!(!third.from_cache);
    assert_ne!(third.path, first.path);
    assert_ne!(std::fs::read(&third.path).unwrap(), std::fs::read(&first.path).unwrap());
}

#[test]
fn recipe_preview_never_touches_the_original() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "b.jpg");
    let before = std::fs::read(&src).unwrap();
    let before_mtime = std::fs::metadata(&src).unwrap().modified().unwrap();

    recipe_preview(&src, &tmp.path().join("cache"), BW, 512, 85).unwrap();

    assert_eq!(std::fs::read(&src).unwrap(), before, "originals are read-only (PLAN §3 rule 1)");
    assert_eq!(std::fs::metadata(&src).unwrap().modified().unwrap(), before_mtime);
}

#[test]
fn recipe_preview_renders_within_the_edge_budget() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "c.jpg");
    let out = recipe_preview(&src, &tmp.path().join("cache"), BW, 8, 85).unwrap();
    let dims = image::image_dimensions(&out.path).unwrap();
    assert!(dims.0 <= 8, "longest edge must respect max_edge, got {dims:?}");
}

#[test]
fn recipe_preview_rejects_a_malformed_recipe() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "d.jpg");
    let err = recipe_preview(&src, &tmp.path().join("cache"), "{not json", 512, 85);
    assert!(err.is_err(), "a malformed recipe must fail loudly, not render the original");
}

#[tokio::test]
async fn recipe_preview_job_completes_through_the_queue() {
    use fotobase_jobs::Outcome;
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "e.jpg");
    let job = fotobase_render::recipe_preview_job(
        src.clone(),
        tmp.path().join("cache"),
        BW.to_string(),
        512,
        85,
    );
    let queue = fotobase_jobs::Queue::new(2);
    let handle = queue.enqueue("test-key", fotobase_jobs::Priority::Visible, job).await.unwrap();
    match &*handle.result().await {
        Outcome::Done(Ok(p)) => assert!(p.path.exists()),
        other => panic!("expected Done(Ok), got {other:?}"),
    }
}

#[test]
fn unknown_ops_are_skipped_not_fatal() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "f.jpg");
    let with_unknown = r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"curves","params":{}},{"type":"bw","params":{"amount":1.0}}]}"#;
    let out = recipe_preview(&src, &tmp.path().join("cache"), with_unknown, 512, 85);
    assert!(out.is_ok(), "reader conformance (spec §5): unknown ops are skipped");
    let _ = recipe::parse(with_unknown).unwrap();
}
