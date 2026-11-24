//! TASK-09-005: share sheet path. The selected photos are rendered into a
//! scratch folder (edits baked in by the shared export engine, originals
//! untouched) and the resulting files go to the platform share API in one
//! call; a fake target stands in for the native sheet.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use fotobase_catalog::Catalog;
use fotobase_lib::share_sheet::{hand_off, prepare_share, ShareError, ShareTarget, SHARE_MAX_EDGE};

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

/// Records every call, like the native sheet would receive them.
#[derive(Default)]
struct FakeSheet {
    calls: RefCell<Vec<Vec<PathBuf>>>,
    fail: bool,
}

impl ShareTarget for FakeSheet {
    fn share_files(&self, files: &[PathBuf]) -> Result<(), String> {
        if self.fail {
            return Err("sheet refused".into());
        }
        self.calls.borrow_mut().push(files.to_vec());
        Ok(())
    }
}

#[test]
fn multi_select_hands_every_file_to_one_share_call_in_selection_order() {
    let lib = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let a = gen_jpeg(lib.path(), "a.jpg", 800, 600, [255, 0, 0]);
    let b = gen_jpeg(lib.path(), "b.jpg", 600, 800, [0, 255, 0]);
    let c = gen_jpeg(lib.path(), "c.jpg", 640, 480, [0, 0, 255]);
    let (cat, ids) =
        catalog_with_photos(lib.path(), &[(a, 800, 600), (b, 600, 800), (c, 640, 480)]);
    // Selection order differs from catalog order.
    let picked = [ids[2], ids[0], ids[1]];

    let prepared = prepare_share(&cat, &picked, root.path(), &mut |_, _| {}).unwrap();
    let sheet = FakeSheet::default();
    hand_off(&prepared, &sheet).unwrap();

    let calls = sheet.calls.borrow();
    assert_eq!(calls.len(), 1, "one sheet invocation for the whole selection");
    let names: Vec<_> =
        calls[0].iter().map(|f| f.file_name().unwrap().to_str().unwrap().to_owned()).collect();
    assert_eq!(names, ["c.jpg", "a.jpg", "b.jpg"]);
    for f in &calls[0] {
        assert!(f.starts_with(root.path()) && f.is_file(), "attached file exists in scratch");
    }
}

#[test]
fn single_photo_is_rendered_and_never_upscaled() {
    let lib = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let src = gen_jpeg(lib.path(), "tiny.jpg", 400, 300, [10, 20, 30]);
    let before = std::fs::read(&src).unwrap();
    let (cat, ids) = catalog_with_photos(lib.path(), &[(src.clone(), 400, 300)]);

    let prepared = prepare_share(&cat, &ids, root.path(), &mut |_, _| {}).unwrap();
    let files = prepared.files();

    assert_eq!(files.len(), 1);
    assert_eq!(image::image_dimensions(&files[0]).unwrap(), (400, 300));
    assert_eq!(std::fs::read(&src).unwrap(), before, "original is byte-identical");
}

#[test]
fn oversized_photo_is_capped_to_the_share_edge() {
    let lib = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let src = gen_jpeg(lib.path(), "big.jpg", 6000, 4000, [200, 120, 40]);
    let (cat, ids) = catalog_with_photos(lib.path(), &[(src, 6000, 4000)]);

    let prepared = prepare_share(&cat, &ids, root.path(), &mut |_, _| {}).unwrap();

    let (w, h) = image::image_dimensions(&prepared.files()[0]).unwrap();
    assert_eq!(w.max(h), SHARE_MAX_EDGE);
    assert_eq!((w, h), (4096, 2731), "aspect ratio kept");
}

#[test]
fn unreadable_photo_is_skipped_and_reported() {
    let lib = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let a = gen_jpeg(lib.path(), "a.jpg", 300, 200, [1, 2, 3]);
    let gone = lib.path().join("gone.jpg");
    let c = gen_jpeg(lib.path(), "c.jpg", 300, 200, [4, 5, 6]);
    let (cat, ids) =
        catalog_with_photos(lib.path(), &[(a, 300, 200), (gone, 10, 10), (c, 300, 200)]);

    let mut ticks = Vec::new();
    let prepared = prepare_share(&cat, &ids, root.path(), &mut |d, t| ticks.push((d, t))).unwrap();

    assert_eq!(prepared.files().len(), 2);
    assert_eq!(prepared.failed.len(), 1);
    assert_eq!(prepared.failed[0].file_id, ids[1]);
    assert_eq!(ticks.last(), Some(&(3, 3)));
}

#[test]
fn nothing_rendered_is_an_error_and_leaves_no_scratch_folder() {
    let lib = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let gone = lib.path().join("gone.jpg");
    let (cat, ids) = catalog_with_photos(lib.path(), &[(gone, 10, 10)]);

    let err = prepare_share(&cat, &ids, root.path(), &mut |_, _| {}).unwrap_err();

    assert!(matches!(err, ShareError::NothingExported));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn share_target_failure_surfaces() {
    let lib = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let a = gen_jpeg(lib.path(), "a.jpg", 300, 200, [1, 2, 3]);
    let (cat, ids) = catalog_with_photos(lib.path(), &[(a, 300, 200)]);
    let prepared = prepare_share(&cat, &ids, root.path(), &mut |_, _| {}).unwrap();

    let err = hand_off(&prepared, &FakeSheet { fail: true, ..FakeSheet::default() }).unwrap_err();

    assert!(matches!(err, ShareError::Sheet(m) if m == "sheet refused"));
}
