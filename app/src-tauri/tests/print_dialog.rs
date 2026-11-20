//! TASK-08-004: the print dialog's backend — the page plan resolved from
//! the catalog (copies, edited aspect, text line) and the page rasters
//! written at print resolution.

use std::path::{Path, PathBuf};

use fotobase_catalog::Catalog;
use fotobase_lib::print_dialog::{prepare_print, print_pages, print_plan, PrintRequest};
use fotobase_render::print::{FitMode, PrintLayout, PrintOptions, TextOptions, TextSource};

fn gen_jpeg(dir: &Path, name: &str, w: u32, h: u32, rgb: [u8; 3]) -> PathBuf {
    let p = dir.join(name);
    let buf: Vec<u8> = (0..w * h).flat_map(|_| rgb).collect();
    let mut out = Vec::new();
    {
        use image::ImageEncoder as _;
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92);
        enc.write_image(&buf, w, h, image::ExtendedColorType::Rgb8).unwrap();
    }
    std::fs::write(&p, &out).unwrap();
    p
}

fn catalog_with_photos(dir: &Path, photos: &[(PathBuf, u32, u32)]) -> (Catalog, Vec<i64>) {
    let mut cat = Catalog::open(dir.join("catalog.db")).unwrap();
    let folder = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    let mut ids = Vec::new();
    for (src, w, h) in photos {
        cat.conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, width, height, media_type)
                 VALUES (?1, ?2, ?3, 10, 0.0, ?4, ?5, 'image')",
                rusqlite::params![
                    folder,
                    src.to_string_lossy(),
                    src.file_name().unwrap().to_str().unwrap(),
                    w,
                    h
                ],
            )
            .unwrap();
        ids.push(cat.conn().last_insert_rowid());
    }
    (cat, ids)
}

fn request(copies: u32) -> PrintRequest {
    PrintRequest {
        copies,
        options: PrintOptions { layout: PrintLayout::L4x6, ..PrintOptions::default() },
    }
}

#[test]
fn plan_repeats_each_photo_per_copy_and_labels_by_source() {
    let dir = tempfile::tempdir().unwrap();
    let a = gen_jpeg(dir.path(), "a.jpg", 300, 200, [200, 40, 40]);
    let b = gen_jpeg(dir.path(), "b.jpg", 200, 300, [40, 80, 200]);
    let (cat, ids) = catalog_with_photos(dir.path(), &[(a, 300, 200), (b, 200, 300)]);

    let mut req = request(2);
    req.options.text = TextOptions { source: TextSource::Filename, ..TextOptions::default() };
    let plan = print_plan(&cat, &ids, &req).unwrap();
    // 2 photos × 2 copies on 4×6 (two cells per letter sheet).
    assert_eq!(plan.pages.len(), 2);
    let planned: Vec<i64> =
        plan.pages.iter().flat_map(|p| p.cells.iter().map(|c| c.file_id)).collect();
    assert_eq!(planned, [ids[0], ids[0], ids[1], ids[1]]);
    assert!(plan.pages.iter().flat_map(|p| &p.cells).all(|c| c.text.is_some()), "filename band");
    assert_eq!(plan.photos.len(), 2, "one preview source per photo, not per copy");
    assert_eq!(plan.photos[0].filename, "a.jpg");
}

#[test]
fn plan_uses_the_edited_aspect() {
    let dir = tempfile::tempdir().unwrap();
    let a = gen_jpeg(dir.path(), "a.jpg", 300, 200, [200, 40, 40]);
    let (mut cat, ids) = catalog_with_photos(dir.path(), &[(a, 300, 200)]);
    let mut req = request(1);
    req.options.fit = FitMode::Crop;

    let plain = print_plan(&cat, &ids, &req).unwrap();
    assert!(plain.pages[0].cells[0].rotated, "landscape photo turns into the portrait cell");

    // Cropping to the left half makes the photo portrait: no rotation.
    let crop = r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"crop","params":{"x":0.0,"y":0.0,"w":0.5,"h":1.0}}]}"#;
    cat.set_recipe(ids[0], crop).unwrap();
    let cropped = print_plan(&cat, &ids, &req).unwrap();
    assert!(!cropped.pages[0].cells[0].rotated);
    assert_eq!(cropped.photos[0].recipe, crop, "preview gets the recipe to bake in");
}

#[test]
fn unknown_and_empty_selections_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (cat, _) = catalog_with_photos(dir.path(), &[]);
    assert!(print_plan(&cat, &[], &request(1)).is_err());
    assert!(print_plan(&cat, &[999], &request(1)).is_err());
}

#[test]
fn pages_are_written_at_print_resolution_and_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let a = gen_jpeg(dir.path(), "a.jpg", 300, 200, [200, 40, 40]);
    let b = gen_jpeg(dir.path(), "b.jpg", 200, 300, [40, 80, 200]);
    let (cat, ids) = catalog_with_photos(dir.path(), &[(a, 300, 200), (b, 200, 300)]);
    let out = tempfile::tempdir().unwrap();

    let job = prepare_print(&cat, &ids, &request(2)).unwrap();
    let mut done = Vec::new();
    let batch = print_pages(&job, out.path(), &mut |n| done.push(n)).unwrap();
    assert!(batch.failed.is_empty());
    assert_eq!(batch.files.len(), 2);
    assert_eq!(done, [1, 2]);
    for f in &batch.files {
        let img = image::open(f).unwrap();
        assert_eq!((img.width(), img.height()), (2550, 3300), "8.5×11 in at 300 dpi");
    }

    let again = print_pages(&job, out.path(), &mut |_| {}).unwrap();
    assert!(again.files.iter().all(|f| !batch.files.contains(f)), "second print keeps the first");

    assert!(print_pages(&job, &out.path().join("missing"), &mut |_| {}).is_err());
}
