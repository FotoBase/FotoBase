//! Info pane data (TASK-01-020, PLAN §2D): one photo's file properties
//! (path, size, dimensions, format, modified time) plus the EXIF fields
//! cached in the `metadata` table at scan time. A `LEFT JOIN` keeps photos
//! without a metadata row listed — the pane shows blanks, never errors.

use fotobase_catalog::Catalog;
use rusqlite::params;
use tempfile::TempDir;

fn open_in(dir: &TempDir) -> Catalog {
    Catalog::open(dir.path().join("catalog.db")).expect("open catalog")
}

/// Insert a file row directly (scanner-shaped).
fn add_file(cat: &mut Catalog, folder: i64, path: &str, size: i64) -> i64 {
    let filename = path.rsplit('/').next().unwrap().to_string();
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, hash, width, height, media_type)
             VALUES (?1, ?2, ?3, ?4, 1760000000.0, NULL, 6000, 4000, 'image')",
            params![folder, path, filename, size],
        )
        .unwrap();
    cat.conn().query_row("SELECT last_insert_rowid()", [], |r| r.get(0)).unwrap()
}

/// Cache a scan-time EXIF payload in the `metadata` table (TASK-01-020
/// technical spec: values land there where available).
fn add_metadata(cat: &mut Catalog, file_id: i64, camera: &str, lens: &str, f: f64, iso: i64) {
    cat.conn()
        .execute(
            "INSERT INTO metadata(file_id, camera, lens, f_number, iso) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![file_id, camera, lens, f, iso],
        )
        .unwrap();
}

#[test]
fn photo_info_returns_file_properties_and_exif() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let folder = cat.seed_folder("/photos").unwrap();
    let id = add_file(&mut cat, folder, "/photos/beach.jpg", 1_234_567);
    add_metadata(&mut cat, id, "Canon EOS R5", "RF 24-70mm F2.8", 2.8, 100);

    let info = cat.photo_info(id).unwrap().expect("row exists");
    // File properties (PLAN §2D: path, size, dimensions, format, modified).
    assert_eq!(info.path, "/photos/beach.jpg");
    assert_eq!(info.filename, "beach.jpg");
    assert_eq!(info.size, 1_234_567);
    assert_eq!(info.width, Some(6000));
    assert_eq!(info.height, Some(4000));
    assert_eq!(info.media_type, "image");
    assert!((info.mtime - 1_760_000_000.0).abs() < f64::EPSILON);
    // EXIF from the metadata table.
    assert_eq!(info.camera.as_deref(), Some("Canon EOS R5"));
    assert_eq!(info.lens.as_deref(), Some("RF 24-70mm F2.8"));
    assert_eq!(info.f_number, Some(2.8));
    assert_eq!(info.iso, Some(100));
}

#[test]
fn photo_without_metadata_row_reports_null_exif() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let folder = cat.seed_folder("/photos").unwrap();
    let id = add_file(&mut cat, folder, "/photos/scan.jpg", 42);

    let info = cat.photo_info(id).unwrap().expect("row exists");
    assert_eq!(info.camera, None);
    assert_eq!(info.lens, None);
    assert_eq!(info.f_number, None);
    assert_eq!(info.iso, None);
    // File properties are still present.
    assert_eq!(info.path, "/photos/scan.jpg");
    assert_eq!(info.size, 42);
}

#[test]
fn photo_info_for_unknown_id_is_none() {
    let dir = TempDir::new().unwrap();
    let cat = open_in(&dir);
    assert!(cat.photo_info(999).unwrap().is_none());
}
