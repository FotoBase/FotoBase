//! TASK-06-002: the map's pin data. Every non-hidden photo with a complete
//! GPS mirror (both latitude and longitude, from the sidecar — the source
//! of truth) comes back exactly once, keyed by its content hash so the
//! frontend can build `thumb://` URIs. Photos without GPS, hidden photos,
//! and photos inside hidden folders never appear.

use fotobase_catalog::Catalog;

fn photo(
    cat: &mut Catalog,
    dir: &std::path::Path,
    name: &str,
    gps: Option<(f64, f64)>,
    hidden: bool,
) -> i64 {
    let path = dir.join(name);
    let folder_id = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, hash, media_type, hidden)
             VALUES (?1, ?2, ?3, 10, 0.0, ?4, 'image', ?5)",
            rusqlite::params![
                folder_id,
                path.to_str().unwrap(),
                name,
                format!("hash-{name}"),
                hidden as i64,
            ],
        )
        .unwrap();
    let id = cat.conn().last_insert_rowid();
    if let Some((lat, lon)) = gps {
        cat.conn()
            .execute(
                "INSERT INTO metadata (file_id, latitude, longitude) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, lat, lon],
            )
            .unwrap();
    }
    id
}

#[test]
fn returns_every_geotagged_photo_once_with_its_hash() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    photo(&mut cat, dir.path(), "a.jpg", Some((52.52, 13.405)), false);
    photo(&mut cat, dir.path(), "b.jpg", Some((-33.86, 151.21)), false);
    photo(&mut cat, dir.path(), "c.jpg", Some((48.85, 2.35)), false);

    let pins = cat.geotagged_pins().unwrap();
    assert_eq!(pins.len(), 3, "{pins:?}");
    let a = pins.iter().find(|p| p.hash == "hash-a.jpg").unwrap();
    assert_eq!(a.hash, "hash-a.jpg");
    assert!((a.latitude - 52.52).abs() < 1e-9);
    assert!((a.longitude - 13.405).abs() < 1e-9);
}

#[test]
fn skips_photos_without_gps_and_without_a_hash() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    photo(&mut cat, dir.path(), "no-gps.jpg", None, false);
    photo(&mut cat, dir.path(), "half-gps.jpg", Some((1.0, f64::NAN)), false);
    // A metadata row with only one coordinate set is incomplete.
    let id = photo(&mut cat, dir.path(), "lon-only.jpg", None, false);
    cat.conn().execute("UPDATE metadata SET longitude = 13.4 WHERE file_id = ?1", [id]).unwrap();

    let pins = cat.geotagged_pins().unwrap();
    assert!(pins.is_empty(), "{pins:?}");
}

#[test]
fn skips_hidden_photos_and_photos_in_hidden_folders() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    photo(&mut cat, dir.path(), "hidden.jpg", Some((1.0, 2.0)), true);
    let folder = dir.path().join("secret");
    std::fs::create_dir(&folder).unwrap();
    photo(&mut cat, &folder, "in-hidden.jpg", Some((3.0, 4.0)), false);
    cat.conn()
        .execute(
            "UPDATE folders SET hidden = 1, hidden_eff = 1 WHERE path = ?1",
            [folder.to_str().unwrap()],
        )
        .unwrap();

    let pins = cat.geotagged_pins().unwrap();
    assert!(pins.is_empty(), "{pins:?}");
}

#[test]
fn half_gps_is_never_returned_even_when_the_other_axis_is_set() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    photo(&mut cat, dir.path(), "lat-only.jpg", Some((10.0, 0.0)), false);
    cat.conn()
        .execute("UPDATE files SET hash = NULL WHERE hash = ?1", ["hash-lat-only.jpg"])
        .unwrap();

    assert!(cat.geotagged_pins().unwrap().is_empty());
}
