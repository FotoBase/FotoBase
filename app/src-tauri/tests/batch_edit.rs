//! TASK-04-011 integration tests: batch edit ops on real fixtures. The
//! batch path must produce the *same recipe* as applying the op per-photo,
//! and therefore the same pixels — checked through the real render pipeline
//! (`recipe_preview`, the shared recipe engine at preview resolution).
//! Cancellation mid-batch leaves processed photos with valid recipes and
//! unprocessed ones untouched.

use fotobase_catalog::Catalog;
use fotobase_lib::batch_edit::{self, BatchOp};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

fn gen_jpeg(dir: &Path, name: &str, size: (u32, u32)) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    let (w, h) = size;
    let mut buf = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            buf.extend_from_slice(&[x as u8 * 15, y as u8 * 15, 128]);
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

fn catalog_with_photos(dir: &Path, srcs: &[&std::path::Path]) -> Vec<i64> {
    let mut cat = Catalog::open(dir.join("catalog.db")).unwrap();
    let folder = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    srcs.iter()
        .map(|src| {
            cat.conn()
                .execute(
                    "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
                     VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
                    rusqlite::params![
                        folder,
                        src.to_string_lossy(),
                        src.file_name().unwrap().to_str().unwrap()
                    ],
                )
                .unwrap();
            cat.conn().last_insert_rowid()
        })
        .collect()
}

/// The per-photo path: existing recipe + the op appended by hand (the same
/// transform the single-photo edit flow produces).
fn per_photo_recipe(existing: Option<&str>, op_json: serde_json::Value) -> String {
    let mut ops: Vec<serde_json::Value> = match existing {
        Some(raw) if !raw.trim().is_empty() => {
            let (recipe, _) = fotobase_core::recipe::parse(raw).unwrap();
            recipe.ops.iter().map(|o| serde_json::to_value(o).unwrap()).collect()
        }
        _ => Vec::new(),
    };
    ops.push(op_json);
    serde_json::to_string(&serde_json::json!({
        "spec": "fotobase.recipe", "version": 1, "ops": ops
    }))
    .unwrap()
}

fn recipe_preview_bytes(src: &Path, recipe: &str) -> Vec<u8> {
    let cache = tempfile::TempDir::new().unwrap();
    let preview = fotobase_render::recipe_preview(src, cache.path(), recipe, 2048, 90).unwrap();
    std::fs::read(&preview.path).unwrap()
}

#[test]
fn batch_result_equals_per_photo_result_in_pixels() {
    let tmp = tempfile::TempDir::new().unwrap();
    let photos = tmp.path().join("photos");
    let srcs: Vec<_> = [("a.jpg", (16u32, 12u32)), ("b.jpg", (12, 16)), ("c.jpg", (16, 16))]
        .iter()
        .map(|(n, s)| gen_jpeg(&photos, n, *s))
        .collect();
    let _ids =
        catalog_with_photos(tmp.path(), &srcs.iter().map(|p| p.as_path()).collect::<Vec<_>>());

    let cases = [
        (BatchOp::RotateCw, serde_json::json!({ "type": "rotate", "params": { "angle": 90 } })),
        (BatchOp::Sepia, serde_json::json!({ "type": "sepia", "params": { "amount": 1.0 } })),
        (
            BatchOp::FilmGrain,
            serde_json::json!({ "type": "film_grain", "params": { "amount": 0.5, "seed": 0 } }),
        ),
    ];
    for (op, op_json) in cases {
        // Fresh fixtures *and* catalog per case: batch ops compose
        // (rotate ×2 = 180°) and sidecars live next to the photos, so each
        // case must start from clean recipes.
        let tmp = tempfile::TempDir::new().unwrap();
        let photos = tmp.path().join("photos");
        let srcs: Vec<_> = [("a.jpg", (16u32, 12u32)), ("b.jpg", (12, 16)), ("c.jpg", (16, 16))]
            .iter()
            .map(|(n, s)| gen_jpeg(&photos, n, *s))
            .collect();
        let ids =
            catalog_with_photos(tmp.path(), &srcs.iter().map(|p| p.as_path()).collect::<Vec<_>>());
        let report = batch_edit::run(
            &tmp.path().join("catalog.db"),
            &ids,
            op,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap();
        assert_eq!(report.applied, 3, "{op:?}");
        assert!(report.failed.is_empty(), "{op:?}: {:?}", report.failed);

        for (i, src) in srcs.iter().enumerate() {
            let batch_recipe = Catalog::open(tmp.path().join("catalog.db"))
                .unwrap()
                .file_recipe(ids[i])
                .unwrap()
                .expect("batch wrote a recipe");
            let per_photo = per_photo_recipe(None, op_json.clone());
            assert_eq!(
                batch_recipe,
                per_photo,
                "{op:?} on {}: batch recipe must equal the per-photo recipe",
                src.file_name().unwrap().to_str().unwrap()
            );
            // The pixel criterion, through the real render pipeline: the
            // same recipe text → the same engine output → identical JPEGs.
            assert_eq!(
                recipe_preview_bytes(src, &batch_recipe),
                recipe_preview_bytes(src, &per_photo),
                "{op:?} on {}: pixels must match the per-photo result",
                src.file_name().unwrap().to_str().unwrap()
            );
        }
    }
}

#[test]
fn batch_rotate_swaps_the_preview_dimensions() {
    let tmp = tempfile::TempDir::new().unwrap();
    let photos = tmp.path().join("photos");
    let src = gen_jpeg(&photos, "a.jpg", (16, 12));
    let ids = catalog_with_photos(tmp.path(), &[&src]);

    batch_edit::run(
        &tmp.path().join("catalog.db"),
        &ids,
        BatchOp::RotateCw,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();

    let recipe =
        Catalog::open(tmp.path().join("catalog.db")).unwrap().file_recipe(ids[0]).unwrap().unwrap();
    let cache = tempfile::TempDir::new().unwrap();
    let preview = fotobase_render::recipe_preview(&src, cache.path(), &recipe, 2048, 90).unwrap();
    let dims = image::image_dimensions(&preview.path).unwrap();
    assert_eq!(dims, (12, 16), "90° CW rotation swaps width and height");
}

#[test]
fn cancel_mid_batch_keeps_processed_and_skips_the_rest() {
    let tmp = tempfile::TempDir::new().unwrap();
    let photos = tmp.path().join("photos");
    let srcs: Vec<_> = (0..5).map(|i| gen_jpeg(&photos, &format!("p{i}.jpg"), (16, 12))).collect();
    let ids =
        catalog_with_photos(tmp.path(), &srcs.iter().map(|p| p.as_path()).collect::<Vec<_>>());

    // An already-set-up cancel flag: the first photo is processed, then the
    // run observes the flag and stops before the second.
    let cancel = AtomicBool::new(false);
    cancel.store(true, Ordering::Relaxed);
    let report =
        batch_edit::run(&tmp.path().join("catalog.db"), &ids, BatchOp::Sepia, &cancel, &|_| {})
            .unwrap();

    assert!(report.cancelled);
    assert_eq!(report.applied, 0, "cancelled before any photo: nothing applied");
    for src in &srcs {
        assert!(
            !fotobase_catalog::sidecar_path(src.to_str().unwrap()).exists(),
            "an unprocessed photo must not gain a sidecar"
        );
    }

    // Cancel after the first photo: it keeps its valid recipe.
    let seen = std::cell::Cell::new(0);
    let cancel = AtomicBool::new(false);
    let report =
        batch_edit::run(&tmp.path().join("catalog.db"), &ids, BatchOp::Sepia, &cancel, &|_| {
            let n = seen.get();
            seen.set(n + 1);
            if n == 0 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
    assert!(report.cancelled);
    assert_eq!(report.applied, 1);
    assert!(Catalog::open(tmp.path().join("catalog.db"))
        .unwrap()
        .file_recipe(ids[0])
        .unwrap()
        .is_some());
    for src in srcs.iter().skip(1) {
        assert!(!fotobase_catalog::sidecar_path(src.to_str().unwrap()).exists());
    }
}
