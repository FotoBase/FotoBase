//! TASK-06-002: the map's pin data. Every non-hidden photo with a complete
//! GPS mirror (both latitude and longitude, from the sidecar — the source
//! of truth) comes back exactly once, keyed by its content hash so the
//! frontend can build `thumb://` URIs. Photos without GPS, hidden photos,
//! and photos inside hidden folders never appear.

use fotobase_catalog::{Catalog, FileQuery};

// TASK-06-003: the filtered pin query. The map must show exactly the pins
// the current search/filter view selects — the same WHERE the grid's
// `query_files` compiles, applied to the geotagged set (PLAN §2 View menu
// "Map", §6 P3).

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

#[test]
fn no_filters_returns_the_whole_geotagged_library() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    photo(&mut cat, dir.path(), "a.jpg", Some((52.52, 13.405)), false);
    photo(&mut cat, dir.path(), "b.jpg", Some((-33.86, 151.21)), false);
    photo(&mut cat, dir.path(), "plain.jpg", None, false);

    let pins = cat.geotagged_pins_filtered(&FileQuery::default()).unwrap();
    assert_eq!(pins.len(), 2, "{pins:?}");
}

#[test]
fn search_text_narrows_the_pins_by_fts() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    photo(&mut cat, dir.path(), "berlin.jpg", Some((52.52, 13.405)), false);
    photo(&mut cat, dir.path(), "sydney.jpg", Some((-33.86, 151.21)), false);

    let pins = cat
        .geotagged_pins_filtered(&FileQuery { text: Some("berlin".into()), ..FileQuery::default() })
        .unwrap();
    assert_eq!(pins.len(), 1, "{pins:?}");
    assert_eq!(pins[0].hash, "hash-berlin.jpg");
}

#[test]
fn unsearchable_text_yields_no_pins() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    photo(&mut cat, dir.path(), "a.jpg", Some((1.0, 2.0)), false);

    let pins = cat
        .geotagged_pins_filtered(&FileQuery { text: Some("!@#".into()), ..FileQuery::default() })
        .unwrap();
    assert!(pins.is_empty(), "{pins:?}");
}

#[test]
fn folder_filter_narrows_the_pins() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let keep = photo(&mut cat, dir.path(), "keep.jpg", Some((1.0, 2.0)), false);
    let other_dir = tempfile::tempdir().unwrap();
    photo(&mut cat, other_dir.path(), "drop.jpg", Some((3.0, 4.0)), false);
    let folder_id: i64 = cat
        .conn()
        .query_row("SELECT folder_id FROM files WHERE id = ?1", [keep], |r| r.get(0))
        .unwrap();

    let pins = cat
        .geotagged_pins_filtered(&FileQuery {
            folders: Some(vec![folder_id]),
            ..FileQuery::default()
        })
        .unwrap();
    assert_eq!(pins.len(), 1, "{pins:?}");
    assert_eq!(pins[0].hash, "hash-keep.jpg");
}

#[test]
fn min_rating_narrows_the_pins() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let starred = photo(&mut cat, dir.path(), "star.jpg", Some((1.0, 2.0)), false);
    photo(&mut cat, dir.path(), "plain.jpg", Some((3.0, 4.0)), false);
    cat.conn().execute("UPDATE metadata SET rating = 5 WHERE file_id = ?1", [starred]).unwrap();

    let pins = cat
        .geotagged_pins_filtered(&FileQuery { min_rating: Some(5), ..FileQuery::default() })
        .unwrap();
    assert_eq!(pins.len(), 1, "{pins:?}");
    assert_eq!(pins[0].hash, "hash-star.jpg");
}

#[test]
fn videos_only_narrows_the_pins() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let video = photo(&mut cat, dir.path(), "clip.mp4", Some((1.0, 2.0)), false);
    photo(&mut cat, dir.path(), "img.jpg", Some((3.0, 4.0)), false);
    cat.conn().execute("UPDATE files SET media_type = 'video' WHERE id = ?1", [video]).unwrap();

    let pins = cat
        .geotagged_pins_filtered(&FileQuery { videos_only: true, ..FileQuery::default() })
        .unwrap();
    assert_eq!(pins.len(), 1, "{pins:?}");
    assert_eq!(pins[0].hash, "hash-clip.mp4");
}

#[test]
fn taken_since_narrows_the_pins() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    photo(&mut cat, dir.path(), "old.jpg", Some((1.0, 2.0)), false);
    let new_id = photo(&mut cat, dir.path(), "new.jpg", Some((3.0, 4.0)), false);
    cat.conn()
        .execute("UPDATE files SET taken_at = '2024-06-01T12:00:00' WHERE id = ?1", [new_id])
        .unwrap();

    let pins = cat
        .geotagged_pins_filtered(&FileQuery {
            taken_since: Some("2024-01-01".into()),
            ..FileQuery::default()
        })
        .unwrap();
    assert_eq!(pins.len(), 1, "{pins:?}");
    assert_eq!(pins[0].hash, "hash-new.jpg");
}
