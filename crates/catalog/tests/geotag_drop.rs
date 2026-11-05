//! TASK-06-005: drag-to-geotag. Dropping photos on the map geotags them:
//! the GPS lands in the sidecar (source of truth, PLAN §5), the catalog
//! mirror follows, and the original's bytes never change (PLAN §3 rule 1).

use std::path::{Path, PathBuf};

use fotobase_catalog::{sidecar_path, Catalog};
use fotobase_metadata::Sidecar;

fn add_photo(cat: &mut Catalog, dir: &Path, name: &str) -> (i64, PathBuf) {
    let original = dir.join(name);
    std::fs::write(&original, b"fake image bytes").unwrap();
    let folder_id = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, hash, media_type)
             VALUES (?1, ?2, ?3, 10, 0.0, ?4, 'image')",
            rusqlite::params![folder_id, original.to_str().unwrap(), name, format!("hash-{name}")],
        )
        .unwrap();
    (cat.conn().last_insert_rowid(), original)
}

#[test]
fn geotagging_writes_the_sidecar_and_mirror_and_leaves_originals_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (a, a_path) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (b, b_path) = add_photo(&mut cat, dir.path(), "b.jpg");
    let before_a = std::fs::read(&a_path).unwrap();
    let before_b = std::fs::read(&b_path).unwrap();

    cat.geotag_photos(&[a, b], 52.5200, -13.405).unwrap();

    for (id, path) in [(&a, &a_path), (&b, &b_path)] {
        let sc = Sidecar::read(&sidecar_path(path.to_str().unwrap())).unwrap();
        let (lat, lon) = sc.gps().unwrap();
        assert!((lat - 52.52).abs() < 1e-6, "id {id}: {lat}");
        assert!((lon + 13.405).abs() < 1e-6, "id {id}: {lon}");
        assert_eq!(std::fs::read(path).unwrap(), b"fake image bytes");
    }
    assert_eq!(std::fs::read(&a_path).unwrap(), before_a, "original untouched");
    assert_eq!(std::fs::read(&b_path).unwrap(), before_b, "original untouched");

    let pins = cat.geotagged_pins().unwrap();
    assert_eq!(pins.len(), 2, "{pins:?}");
}

#[test]
fn geotagging_out_of_range_coordinates_is_refused_with_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (a, a_path) = add_photo(&mut cat, dir.path(), "a.jpg");

    assert!(cat.geotag_photos(&[a], 91.0, 0.0).is_err());
    assert!(cat.geotag_photos(&[a], 0.0, 180.5).is_err());
    assert!(!sidecar_path(a_path.to_str().unwrap()).exists(), "no sidecar written");
    assert!(cat.geotagged_pins().unwrap().is_empty());
}
