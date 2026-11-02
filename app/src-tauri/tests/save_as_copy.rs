//! TASK-04-017: "Save as…" and "Save a copy" (PLAN §2F, §6 P2b). Both
//! render the edited pixels into a *new* file and never touch the original
//! (§9 Data safety — checksum-verified). The new file gets its own sidecar
//! carrying the recipe, and is registered in the catalog so the watcher
//! flow picks it up like any other import.

use std::path::{Path, PathBuf};

use fotobase_catalog::Catalog;
use fotobase_lib::save::{save_as_photo, save_copy_photo};

fn gen_jpeg(dir: &Path, name: &str) -> PathBuf {
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

fn catalog_with_photo(dir: &Path, src: &Path) -> (Catalog, i64) {
    let mut cat = Catalog::open(dir.join("catalog.db")).unwrap();
    let folder = cat.seed_folder(dir.to_str().unwrap()).unwrap();
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
    let id = cat.conn().last_insert_rowid();
    (cat, id)
}

const BW: &str =
    r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"bw","params":{"amount":1.0}}]}"#;

/// RMS of per-channel deltas, 0–255 scale.
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

fn decode_rgb(path: &Path) -> Vec<u8> {
    image::open(path).expect("file decodes").to_rgb8().into_raw()
}

fn catalog_row_id(cat: &Catalog, path: &Path) -> Option<i64> {
    cat.conn()
        .query_row("SELECT id FROM files WHERE path = ?1", [path.to_string_lossy().as_ref()], |r| {
            r.get(0)
        })
        .ok()
}

#[test]
fn save_as_writes_edited_pixels_to_a_new_file_and_never_touches_the_original() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let original_bytes = std::fs::read(&src).unwrap();
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    cat.set_recipe(id, BW).unwrap();

    let dest = tmp.path().join("elsewhere").join("edited.jpg");
    let saved = save_as_photo(&mut cat, id, &dest).unwrap();

    // New file exists with edited (B&W) pixels — far from the original.
    let new_pixels = decode_rgb(&saved.path);
    assert!(rms(&new_pixels, &decode_rgb(&src)) > 20.0, "new file must hold edited pixels");
    // The original is byte-identical (§9).
    assert_eq!(std::fs::read(&src).unwrap(), original_bytes, "original must be untouched");
    // The new file is cataloged.
    assert_eq!(catalog_row_id(&cat, &saved.path), Some(saved.file_id));
}

#[test]
fn save_a_copy_defaults_next_to_the_original_with_a_copy_suffix() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let original_bytes = std::fs::read(&src).unwrap();
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    cat.set_recipe(id, BW).unwrap();

    let saved = save_copy_photo(&mut cat, id, None).unwrap();

    assert_eq!(
        saved.path,
        src.parent().unwrap().join("a (copy).jpg"),
        "default destination is a sibling with a copy suffix"
    );
    assert!(saved.path.exists());
    assert!(rms(&decode_rgb(&saved.path), &decode_rgb(&src)) > 20.0);
    assert_eq!(std::fs::read(&src).unwrap(), original_bytes, "original must be untouched");
}

#[test]
fn destination_collision_never_overwrites_and_auto_suffixes() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    cat.set_recipe(id, BW).unwrap();

    // An innocent bystander file at the chosen destination.
    let dest = tmp.path().join("elsewhere").join("edited.jpg");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::write(&dest, b"do not touch me").unwrap();

    let saved = save_as_photo(&mut cat, id, &dest).unwrap();
    assert_eq!(saved.path, dest.parent().unwrap().join("edited (2).jpg"));
    assert_eq!(std::fs::read(&dest).unwrap(), b"do not touch me", "collision must never overwrite");

    // A second save into the same folder keeps suffixing instead of
    // clobbering either earlier file.
    let saved2 = save_as_photo(&mut cat, id, &dest).unwrap();
    assert_eq!(saved2.path, dest.parent().unwrap().join("edited (3).jpg"));
    assert_eq!(std::fs::read(&dest).unwrap(), b"do not touch me");
    assert!(saved.path.exists());
}

#[test]
fn save_a_copy_collision_suffixes_past_the_copy_name() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    cat.set_recipe(id, BW).unwrap();

    let first = save_copy_photo(&mut cat, id, None).unwrap();
    let second = save_copy_photo(&mut cat, id, None).unwrap();
    assert_eq!(first.path, src.parent().unwrap().join("a (copy).jpg"));
    assert_eq!(second.path, src.parent().unwrap().join("a (copy 2).jpg"));
}

#[test]
fn the_new_file_gets_its_own_sidecar_carrying_the_recipe() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    cat.set_recipe(id, BW).unwrap();

    let dest = tmp.path().join("elsewhere").join("edited.jpg");
    let saved = save_as_photo(&mut cat, id, &dest).unwrap();

    let sc_path = fotobase_catalog::sidecar_path(saved.path.to_string_lossy().as_ref());
    assert!(sc_path.exists(), "new file must get its own sidecar");
    let sc = fotobase_metadata::Sidecar::read(&sc_path).unwrap();
    assert_eq!(sc.recipe().as_deref(), Some(BW));
    // And the catalog mirror agrees (the sidecar is the source of truth).
    assert_eq!(cat.file_recipe(saved.file_id).unwrap().as_deref(), Some(BW));
}

#[test]
fn no_recipe_duplicates_the_original_bytes_verbatim() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let original_bytes = std::fs::read(&src).unwrap();
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);

    let saved = save_copy_photo(&mut cat, id, None).unwrap();
    assert_eq!(
        std::fs::read(&saved.path).unwrap(),
        original_bytes,
        "copy of the untouched picture"
    );
    assert_eq!(std::fs::read(&src).unwrap(), original_bytes);
}

#[test]
fn unsupported_destination_formats_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    cat.set_recipe(id, BW).unwrap();

    let dest = tmp.path().join("edited.nef");
    assert!(save_as_photo(&mut cat, id, &dest).is_err());
    assert!(!dest.exists());
    assert!(catalog_row_id(&cat, &dest).is_none());
}

#[test]
fn a_saved_photo_saves_from_its_archived_original_not_from_edited_pixels() {
    // After a §F Save the recipe must not be applied twice: Save as… /
    // Save a copy render from the archived original, like Re-Save does.
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let original_bytes = std::fs::read(&src).unwrap();
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    cat.set_recipe(id, BW).unwrap();
    fotobase_lib::save::save_photo(&mut cat, id).unwrap();

    let saved = save_copy_photo(&mut cat, id, None).unwrap();
    // …and the archive is still the untouched original.
    let archived = src.parent().unwrap().join(".originals").join("a.jpg");
    assert_eq!(std::fs::read(&archived).unwrap(), original_bytes);
    // One B&W pass: far from the untouched original (the archived bytes)…
    assert!(rms(&decode_rgb(&saved.path), &decode_rgb(&archived)) > 20.0);
}
