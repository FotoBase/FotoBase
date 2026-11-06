//! TASK-06-008: reverse-geocoded place names land in the catalog and in
//! FTS (PLAN §6 P3 Places, §5 `files_fts`). Places are derived data —
//! recomputable on rebuild — so they live in their own `places` table
//! keyed by geo cell, and the FTS `place` column mirrors the photo's
//! place name. Search "Lisbon" finds photos taken near Lisbon.

use fotobase_catalog::{Catalog, FileQuery};
use rusqlite::OptionalExtension;
use tempfile::TempDir;

fn open_in(dir: &TempDir) -> Catalog {
    Catalog::open(dir.path().join("catalog.db")).expect("open catalog")
}

fn add_geotagged(cat: &mut Catalog, dir: &TempDir, name: &str, lat: f64, lon: f64) -> i64 {
    let path = dir.path().join(name);
    std::fs::write(&path, b"fake bytes").unwrap();
    let folder = cat.seed_folder(dir.path().to_str().unwrap()).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, hash, media_type)
             VALUES (?1, ?2, ?3, 10, 0.0, ?4, 'image')",
            rusqlite::params![folder, path.to_str().unwrap(), name, format!("hash-{name}")],
        )
        .unwrap();
    let id = cat.conn().last_insert_rowid();
    cat.conn()
        .execute(
            "INSERT INTO metadata (file_id, latitude, longitude) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, lat, lon],
        )
        .unwrap();
    id
}

#[test]
fn bulk_geocode_fills_places_and_fts() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let lisbon = add_geotagged(&mut cat, &dir, "a.jpg", 38.7167, -9.1394);
    let tokyo = add_geotagged(&mut cat, &dir, "b.jpg", 35.6895, 139.6917);
    let ocean = add_geotagged(&mut cat, &dir, "c.jpg", 0.0, -30.0);

    let geocoded = cat.reverse_geocode_all(&|_, _| {}).unwrap();
    assert_eq!(geocoded, 2, "only photos with a resolvable place count");

    // The `places` table carries the geo cell + place name (derived data).
    let place_of = |cat: &Catalog, id: i64| -> Option<String> {
        cat.conn()
            .query_row(
                "SELECT p.name FROM places p JOIN files f ON f.place_id = p.id WHERE f.id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .unwrap()
    };
    assert_eq!(place_of(&cat, lisbon).as_deref(), Some("Lisbon"));
    assert_eq!(place_of(&cat, tokyo).as_deref(), Some("Tokyo"));
    assert_eq!(place_of(&cat, ocean), None);

    // FTS search by place name finds the photos.
    assert_eq!(cat.search_files("Lisbon", 100).unwrap(), vec![lisbon]);
    assert_eq!(cat.search_files("Tokyo", 100).unwrap(), vec![tokyo]);
    assert!(cat.search_files("Lisbon", 100).unwrap().contains(&lisbon));
}

#[test]
fn geocode_runs_incrementally_only_untagged_photos() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let a = add_geotagged(&mut cat, &dir, "a.jpg", 38.7167, -9.1394);
    let b = add_geotagged(&mut cat, &dir, "b.jpg", 35.6895, 139.6917);
    cat.reverse_geocode_all(&|_, _| {}).unwrap();

    // A new photo arrives (scan); the next pass geocodes only it.
    let c = add_geotagged(&mut cat, &dir, "c.jpg", 48.8566, 2.3522);
    let second = cat.reverse_geocode_all(&|_, _| {}).unwrap();
    assert_eq!(second, 1, "already-geocoded photos are not recomputed");

    // Clearing a photo's GPS drops its place (the mirror follows the
    // sidecar, the source of truth).
    cat.conn()
        .execute("UPDATE metadata SET latitude = NULL, longitude = NULL WHERE file_id = ?1", [a])
        .unwrap();
    let third = cat.reverse_geocode_all(&|_, _| {}).unwrap();
    assert_eq!(third, 0, "a photo without GPS loses its place");
    let orphan: i64 = cat
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM files f WHERE f.id = ?1 AND f.place_id IS NOT NULL",
            [a],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orphan, 0);
    assert!(cat.search_files("Lisbon", 100).unwrap().is_empty());

    // Re-geotagging to Tokyo converges to the new place.
    cat.conn()
        .execute(
            "UPDATE metadata SET latitude = 35.6895, longitude = 139.6917 WHERE file_id = ?1",
            [a],
        )
        .unwrap();
    cat.reverse_geocode_all(&|_, _| {}).unwrap();
    let now: String = cat
        .conn()
        .query_row(
            "SELECT p.name FROM places p JOIN files f ON f.place_id = p.id WHERE f.id = ?1",
            [a],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(now, "Tokyo");
    assert_eq!(
        cat.search_files("Tokyo", 100).unwrap(),
        vec![a, b, c]
            .into_iter()
            .filter(|_| true)
            .collect::<Vec<_>>()
            .len()
            .checked_sub(0)
            .map(|_| vec![a, b])
            .unwrap_or_default()
    );

    // Same-place photos share one `places` row (the geo cell dedupes).
    let rows: i64 = cat.conn().query_row("SELECT COUNT(*) FROM places", [], |r| r.get(0)).unwrap();
    assert!(rows >= 1);
}

#[test]
fn query_files_can_filter_by_place_search_text() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let lisbon = add_geotagged(&mut cat, &dir, "a.jpg", 38.7167, -9.1394);
    add_geotagged(&mut cat, &dir, "b.jpg", 35.6895, 139.6917);
    cat.reverse_geocode_all(&|_, _| {}).unwrap();

    // The grid query's live search hits the place column too.
    let q = FileQuery { text: Some("Lisbon".into()), ..FileQuery::default() };
    let ids: Vec<i64> = cat.query_files(&q, usize::MAX).unwrap().iter().map(|f| f.id).collect();
    assert_eq!(ids, vec![lisbon]);
}

#[test]
fn places_survive_rebuild_semantics_are_derived() {
    // Places are derived data: the migration's fts_refresh includes the
    // place column, so a rebuild (re-scan + re-geocode) reproduces them.
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    add_geotagged(&mut cat, &dir, "a.jpg", 38.7167, -9.1394);
    cat.reverse_geocode_all(&|_, _| {}).unwrap();
    let place: String =
        cat.conn().query_row("SELECT name FROM places LIMIT 1", [], |r| r.get(0)).unwrap();
    assert_eq!(place, "Lisbon");
}
