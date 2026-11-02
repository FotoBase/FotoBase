//! TASK-04-015: the export dialog's backend batch — numbered filenames
//! sequential and collision-free, edits baked in, resize limits honored on
//! both dimensions, watermark present in the output bytes, failures never
//! stop the rest of the batch.

use std::path::{Path, PathBuf};

use fotobase_catalog::Catalog;
use fotobase_lib::export_dialog::{export_photos, ExportRequest};

fn gen_jpeg(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    let mut buf = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            buf.extend_from_slice(&[(x % 64) as u8 * 4, (y % 32) as u8 * 8, 128]);
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

fn catalog_with_photos(dir: &Path, srcs: &[PathBuf]) -> (Catalog, Vec<i64>) {
    let mut cat = Catalog::open(dir.join("catalog.db")).unwrap();
    let folder = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    let mut ids = Vec::new();
    for src in srcs {
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
        ids.push(cat.conn().last_insert_rowid());
    }
    (cat, ids)
}

const BW: &str =
    r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"bw","params":{"amount":1.0}}]}"#;

fn decode_rgb(path: &Path) -> (Vec<u8>, u32, u32) {
    let img = image::open(path).expect("exported file decodes").to_rgb8();
    let (w, h) = (img.width(), img.height());
    (img.into_raw(), w, h)
}

fn rms(a: &[u8], b: &[u8]) -> f64 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = i64::from(*x) - i64::from(*y);
            (d * d) as f64
        })
        .sum::<f64>()
        .sqrt()
}

fn req() -> ExportRequest {
    ExportRequest {
        resize_max_width: None,
        resize_max_height: None,
        quality: 90,
        numbered: false,
        watermark: None,
    }
}

#[test]
fn numbered_batch_exports_sequential_collision_free_filenames_in_order() {
    let tmp = tempfile::tempdir().unwrap();
    // Two photos share a stem across folders; batch order is the grid sort.
    let a1 = gen_jpeg(&tmp.path().join("f1"), "a.jpg", 16, 12);
    let a2 = gen_jpeg(&tmp.path().join("f2"), "a.jpg", 16, 12);
    let b = gen_jpeg(&tmp.path().join("f1"), "b.jpg", 16, 12);
    let (cat, ids) = catalog_with_photos(tmp.path(), &[a1, a2, b]);
    let dest = tmp.path().join("exports");
    std::fs::create_dir_all(&dest).unwrap();

    let mut r = req();
    r.numbered = true;
    let mut progress = Vec::new();
    let batch = export_photos(&cat, &ids, &dest, &r, &mut |d, t| progress.push((d, t))).unwrap();

    let names: Vec<String> = batch
        .exported
        .iter()
        .map(|e| e.path.file_name().unwrap().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, vec!["a-1.jpg", "a-2.jpg", "b-3.jpg"], "batch order preserved");
    for name in &names {
        assert!(dest.join(name).exists());
    }
    assert_eq!(progress, vec![(1, 3), (2, 3), (3, 3)], "progress after every photo");
    assert!(batch.failed.is_empty());
}

#[test]
fn unnumbered_batch_never_overwrites_and_avoids_collisions() {
    let tmp = tempfile::tempdir().unwrap();
    let a1 = gen_jpeg(&tmp.path().join("f1"), "a.jpg", 16, 12);
    let a2 = gen_jpeg(&tmp.path().join("f2"), "a.jpg", 16, 12);
    let (cat, ids) = catalog_with_photos(tmp.path(), &[a1, a2]);
    let dest = tmp.path().join("exports");
    std::fs::create_dir_all(&dest).unwrap();

    let batch = export_photos(&cat, &ids, &dest, &req(), &mut |_, _| {}).unwrap();
    let names: Vec<String> = batch
        .exported
        .iter()
        .map(|e| e.path.file_name().unwrap().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, vec!["a.jpg", "a-1.jpg"]);
}

#[test]
fn exports_bake_edits_and_honor_resize_limits_and_watermark() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("f1"), "a.jpg", 64, 32);
    let (mut cat, ids) = catalog_with_photos(tmp.path(), std::slice::from_ref(&src));
    cat.set_recipe(ids[0], BW).unwrap();
    let dest = tmp.path().join("exports");
    std::fs::create_dir_all(&dest).unwrap();

    let mut r = req();
    r.resize_max_width = Some(32);
    r.watermark = Some("FOTOBASE".into());
    let batch = export_photos(&cat, &ids, &dest, &r, &mut |_, _| {}).unwrap();
    assert!(batch.failed.is_empty());
    let out = &batch.exported[0].path;

    // Resize limit honored on the horizontal dimension.
    let (pixels, w, h) = decode_rgb(out);
    assert_eq!((w, h), (32, 16));

    // Edits baked in: far from the unedited source pixels.
    let src_pixels = image::open(&src).unwrap().to_rgb8().into_raw();
    assert!(rms(&pixels, &src_pixels) > 20.0, "edits must be baked in");

    // Watermark present in the output bytes: brightened glyph pixels in
    // the bottom-left band.
    let bright =
        pixels.as_chunks::<3>().0.iter().filter(|p| p[0] > 200 && p[1] > 200 && p[2] > 200).count();
    assert!(bright > 0, "watermark glyphs must appear in the output");
}

#[test]
fn portrait_resize_limit_honors_the_vertical_dimension() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("f1"), "p.jpg", 32, 64);
    let (cat, ids) = catalog_with_photos(tmp.path(), &[src]);
    let dest = tmp.path().join("exports");
    std::fs::create_dir_all(&dest).unwrap();

    let mut r = req();
    r.resize_max_height = Some(32);
    let batch = export_photos(&cat, &ids, &dest, &r, &mut |_, _| {}).unwrap();
    let (_, w, h) = decode_rgb(&batch.exported[0].path);
    assert_eq!((w, h), (16, 32));
}

#[test]
fn a_failing_photo_does_not_stop_the_batch_and_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let good = gen_jpeg(&tmp.path().join("f1"), "good.jpg", 16, 12);
    let bad = tmp.path().join("f1").join("bad.jpg"); // never created
    let (cat, ids) = catalog_with_photos(tmp.path(), &[bad, good]);
    let dest = tmp.path().join("exports");
    std::fs::create_dir_all(&dest).unwrap();

    let batch = export_photos(&cat, &ids, &dest, &req(), &mut |_, _| {}).unwrap();
    assert_eq!(batch.exported.len(), 1, "the good photo still exports");
    assert_eq!(batch.exported[0].file_id, ids[1]);
    assert_eq!(batch.failed.len(), 1);
    assert_eq!(batch.failed[0].file_id, ids[0]);
    // The failed file left no partial output behind.
    assert!(!dest.join("bad.jpg").exists());
}
