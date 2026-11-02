//! TASK-04-016: "Save" per the §F spec (PLAN §2F) — the one path that may
//! ever modify an original (§3 rule 1). Save renders the recipe at full
//! resolution into the original path, moves the untouched original to a
//! sibling `.originals/` folder first, and records the saved file as a
//! stacked version (EPIC-03). Revert restores the original byte-identical.

use std::path::{Path, PathBuf};

use fotobase_catalog::Catalog;
use fotobase_lib::save::{revert_photo, save_photo};

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
const SEPIA: &str =
    r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"sepia","params":{"amount":1.0}}]}"#;

fn originals_dir(photo: &Path) -> PathBuf {
    photo.parent().unwrap().join(".originals")
}

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

#[test]
fn save_writes_edited_pixels_archives_the_original_and_stacks() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let original_bytes = std::fs::read(&src).unwrap();
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    cat.set_recipe(id, BW).unwrap();

    save_photo(&mut cat, id).unwrap();

    // Edited pixels now live at the original path: a real image, visibly
    // different from the source.
    let saved = decode_rgb(&src);
    assert_eq!(saved.len(), 16 * 12 * 3, "same dimensions as the original");
    assert!(rms(&saved, &decode_of(&original_bytes)) > 5.0, "pixels changed");

    // The untouched original sits byte-identical in `.originals/` (§9).
    let archived = originals_dir(&src).join("a.jpg");
    assert_eq!(std::fs::read(&archived).unwrap(), original_bytes, "original untouched");

    // The sidecar stayed alongside the saved file and still carries the
    // recipe (the saved file *is* the edited version).
    let sc =
        fotobase_metadata::Sidecar::read(&fotobase_catalog::sidecar_path(src.to_str().unwrap()))
            .unwrap();
    assert!(sc.has_recipe());

    // Stacked version: the grid shows one collapsed stack of two, the saved
    // file on top, the archived original as its (hidden) member.
    let rows = cat.query_files(&fotobase_catalog::FileQuery::default(), 10).unwrap();
    let saved_row = rows.iter().find(|f| f.id == id).expect("saved file still listed");
    assert_eq!(saved_row.stack_count, 2, "saved file recorded as a stacked version");
    let role: String = cat
        .conn()
        .query_row("SELECT stack_role FROM files WHERE id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(role, "top", "the saved file leads its stack");

    // The archived original is cataloged but hidden — it never joins the
    // grid by itself, only as a stack member of the saved photo (the
    // collapsed-stack query lists the top only).
    cat.set_show_hidden(true);
    let members = cat.stack_members(saved_row.stack_id.as_deref().unwrap()).unwrap();
    assert_eq!(members.len(), 2);
    let archived_member = members.iter().find(|m| m.id != id).expect("archived original cataloged");
    assert!(archived_member.hidden, "archived original stays out of the grid");
}

fn decode_of(bytes: &[u8]) -> Vec<u8> {
    image::load_from_memory(bytes).unwrap().to_rgb8().into_raw()
}

#[test]
fn revert_restores_the_original_byte_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let original_bytes = std::fs::read(&src).unwrap();
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    cat.set_recipe(id, BW).unwrap();

    save_photo(&mut cat, id).unwrap();
    revert_photo(&mut cat, id).unwrap();

    // Byte-identical restore, any time (§9 Data safety).
    assert_eq!(std::fs::read(&src).unwrap(), original_bytes);
    // The archive and its catalog row are gone; the photo is unstacked and
    // its recipe cleared (the edits were on the overwritten pixels).
    assert!(originals_dir(&src).read_dir().unwrap().next().is_none());
    let row = cat.query_files(&fotobase_catalog::FileQuery::default(), 10).unwrap();
    assert_eq!(row.len(), 1);
    assert_eq!(row[0].id, id);
    assert_eq!((row[0].stack_id.as_deref(), row[0].stack_count), (None, 0));
    assert!(!row[0].hidden);
    assert_eq!(cat.file_recipe(id).unwrap(), None);
    // Consistent with the existing clear path (TASK-04-010): the sidecar
    // may survive as an fb-less file, but it must no longer carry a recipe.
    let sc =
        fotobase_metadata::Sidecar::read(&fotobase_catalog::sidecar_path(src.to_str().unwrap()))
            .unwrap();
    assert!(!sc.has_recipe());
}

#[test]
fn save_without_a_recipe_refuses_and_touches_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let original_bytes = std::fs::read(&src).unwrap();
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);

    assert!(save_photo(&mut cat, id).is_err());

    assert_eq!(std::fs::read(&src).unwrap(), original_bytes);
    assert!(!originals_dir(&src).exists());
}

#[test]
fn save_refuses_raw_originals() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("photos");
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("a.cr2");
    std::fs::write(&src, b"raw bytes").unwrap();
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    cat.set_recipe(id, BW).unwrap();

    // A Save that would replace a RAW with rendered pixels must refuse:
    // originals are never destroyed, only JPEG/PNG get re-encoded.
    assert!(save_photo(&mut cat, id).is_err());
    assert_eq!(std::fs::read(&src).unwrap(), b"raw bytes");
    assert!(!originals_dir(&src).exists());
}

#[test]
fn a_failed_save_leaves_the_original_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let original_bytes = std::fs::read(&src).unwrap();
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    cat.set_recipe(id, BW).unwrap();

    // `.originals` cannot be created: a regular file occupies the name. The
    // save must fail before anything is moved or written.
    std::fs::write(originals_dir(&src), b"not a directory").unwrap();

    assert!(save_photo(&mut cat, id).is_err());
    assert_eq!(std::fs::read(&src).unwrap(), original_bytes, "original never moved");
    // No stack record was written: the photo is unstacked and its sidecar
    // (from the setup recipe) carries no stack membership.
    let (stack_id, stack_role): (Option<String>, Option<String>) = cat
        .conn()
        .query_row("SELECT stack_id, stack_role FROM files WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((stack_id, stack_role), (None, None));
}

#[test]
fn resaving_renders_from_the_archived_original_not_the_saved_pixels() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    cat.set_recipe(id, BW).unwrap();
    save_photo(&mut cat, id).unwrap();

    // A second save with a different recipe must re-render from the
    // archived original — applying the recipe to already-edited pixels
    // would double-apply it.
    cat.set_recipe(id, SEPIA).unwrap();
    save_photo(&mut cat, id).unwrap();

    let archived = originals_dir(&src).join("a.jpg");
    let reference = tmp.path().join("reference.jpg");
    fotobase_render::export_image(
        &archived,
        &reference,
        SEPIA,
        &fotobase_render::ExportOptions {
            format: fotobase_render::ExportFormat::Jpeg,
            quality: fotobase_lib::save::SAVE_JPEG_QUALITY,
            resize_max_edge: None,
            ..fotobase_render::ExportOptions::default()
        },
    )
    .unwrap();
    let delta = rms(&decode_rgb(&src), &decode_rgb(&reference));
    assert!(delta < 2.0, "re-save must equal one application of the recipe (rms {delta})");
}

#[test]
fn a_completed_save_leaves_no_temp_files_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    cat.set_recipe(id, BW).unwrap();

    save_photo(&mut cat, id).unwrap();

    let strays: Vec<_> = std::fs::read_dir(src.parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(strays.is_empty(), "atomic write leaves no .tmp behind: {strays:?}");
}

#[test]
fn a_rescan_keeps_the_archived_original_cataloged() {
    let tmp = tempfile::tempdir().unwrap();
    let photos = tmp.path().join("photos");
    let src = gen_jpeg(&photos, "a.jpg");
    let (mut cat, id) = catalog_with_photo(&photos, &src);
    cat.set_recipe(id, BW).unwrap();
    save_photo(&mut cat, id).unwrap();

    fotobase_scanner::scan_folder(&mut cat, &photos, &std::sync::atomic::AtomicBool::new(false))
        .unwrap();

    // `.originals/` is app-managed: the walk must not re-add the archived
    // bytes as a duplicate photo, must not record it as a missing file, and
    // must not delete its row as vanished.
    let (count, missing): (i64, i64) = cat
        .conn()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM files f JOIN folders d ON d.id = f.folder_id
                     WHERE d.path LIKE '%' || ?1 || ?2 || '%'),
                    (SELECT COUNT(*) FROM missing_files)",
            rusqlite::params![
                std::path::MAIN_SEPARATOR.to_string(),
                fotobase_catalog::ORIGINALS_DIR
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, 1, "exactly the Save-cataloged archived row");
    assert_eq!(missing, 0, "no missing-file snapshot for the archived original");
    let saved: i64 = cat
        .conn()
        .query_row("SELECT COUNT(*) FROM files WHERE id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(saved, 1);
}
