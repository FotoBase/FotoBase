//! TASK-05-008: named faces persist as MWG `mwg-rs:Regions` XMP (the
//! sidecar is the source of truth for user data, PLAN §5) and survive a
//! catalog delete + rebuild; face embeddings live in the sqlite-vec virtual
//! table keyed to `faces.embedding_id`.

use fotobase_catalog::{sidecar_path, Catalog, FaceRecord};
use fotobase_metadata::Sidecar;

fn open(dir: &std::path::Path) -> Catalog {
    Catalog::open(dir.join("catalog.db")).unwrap()
}

fn add_photo(cat: &mut Catalog, dir: &std::path::Path, name: &str) -> i64 {
    let original = dir.join(name);
    std::fs::write(&original, format!("fake image bytes of {name}")).unwrap();
    let folder_id = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type, width, height)
             VALUES (?1, ?2, ?3, 10, 0.0, 'image', 2000, 1000)",
            rusqlite::params![folder_id, original.to_str().unwrap(), name],
        )
        .unwrap();
    cat.conn().last_insert_rowid()
}

/// One detected face in *pixel* coordinates on the 2000×1000 test image:
/// box (100, 150) 400×500 → normalized centre (0.15, 0.4), size (0.2, 0.5).
fn face() -> FaceRecord {
    FaceRecord {
        x: 100.0,
        y: 150.0,
        width: 400.0,
        height: 500.0,
        landmarks: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
        confidence: 0.9,
    }
}

/// Deterministic 128-dimensional embedding (the SFace dimension).
fn e128(seed: u64) -> Vec<f32> {
    (0..128).map(|i| ((i as u64 ^ seed) % 7) as f32 / 7.0 - 0.5).collect()
}

/// One photo with one detected face, confirmed as "Anna" via the
/// TASK-05-004 naming flow, which is what writes the sidecar region.
fn seed_named(dir: &std::path::Path) -> (Catalog, i64, i64) {
    let mut cat = open(dir);
    let file_id = add_photo(&mut cat, dir, "p0.jpg");
    let vector = e128(1);
    let face_id = cat.record_face(file_id, &face(), Some(&vector)).unwrap();
    cat.conn().execute("INSERT INTO persons (name) VALUES ('Anna')", []).unwrap();
    let person_id = cat.conn().last_insert_rowid();
    cat.confirm_suggestion(face_id, person_id).unwrap();
    (cat, file_id, person_id)
}

#[test]
fn confirming_a_face_writes_a_normalized_mwg_region() {
    let dir = tempfile::tempdir().unwrap();
    let (cat, file_id, _person) = seed_named(dir.path());

    let sc = Sidecar::read(&sidecar_path(&format!("{}/p0.jpg", dir.path().display()))).unwrap();
    let regions = sc.face_regions();
    assert_eq!(regions.len(), 1, "{regions:?}");
    assert_eq!(regions[0].name, "Anna");
    // MWG areas are normalized centre-point + size, not pixel boxes.
    assert!((regions[0].x - 0.15).abs() < 1e-6, "{:?}", regions[0]);
    assert!((regions[0].y - 0.4).abs() < 1e-6, "{:?}", regions[0]);
    assert!((regions[0].w - 0.2).abs() < 1e-6, "{:?}", regions[0]);
    assert!((regions[0].h - 0.5).abs() < 1e-6, "{:?}", regions[0]);
    assert_eq!(cat.faces_for_file(file_id).unwrap().len(), 1);
}

#[test]
fn named_faces_survive_a_catalog_delete_and_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("catalog.db");
    let (cat, _file_id, _person) = seed_named(dir.path());
    drop(cat);

    // Delete the rebuildable cache: the database and its WAL side files
    // (the -wal/-shm files may not exist; only the database itself must).
    for suffix in ["", "-wal", "-shm"] {
        // Missing -wal/-shm files are normal; only the database itself
        // must exist.
        let mut p = db.clone().into_os_string();
        p.push(suffix);
        let _ = std::fs::remove_file(&p);
    }

    // A fresh catalog: the scanner re-seeds folder + file rows, then the
    // sync engine rebuilds user data from the sidecars.
    let mut cat = open(dir.path());
    add_photo(&mut cat, dir.path(), "p0.jpg");
    cat.rebuild_from_sidecars(true).unwrap();

    let faces = cat.faces_for_file(1).unwrap();
    assert_eq!(faces.len(), 1, "the named face is restored: {faces:?}");
    let restored = &faces[0];
    assert!((restored.x - 100.0).abs() < 0.5, "{restored:?}");
    assert!((restored.y - 150.0).abs() < 0.5, "{restored:?}");
    assert!((restored.width - 400.0).abs() < 0.5, "{restored:?}");
    assert!((restored.height - 500.0).abs() < 0.5, "{restored:?}");

    let person_name: String = cat
        .conn()
        .query_row(
            "SELECT p.name FROM faces f JOIN persons p ON p.id = f.person_id WHERE f.id = ?1",
            [restored.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(person_name, "Anna");

    // Derived views follow: person album and FTS.
    let album = cat
        .list_albums()
        .unwrap()
        .into_iter()
        .find(|a| a.person_id.is_some() && a.name == "Anna")
        .expect("auto person album rebuilt");
    assert_eq!(cat.album_file_ids(album.id).unwrap(), vec![1]);
    assert_eq!(cat.search_files("Anna", 100).unwrap(), vec![1]);

    // The sidecar is untouched by a rebuild: still exactly one region.
    let sc = Sidecar::read(&sidecar_path(&format!("{}/p0.jpg", dir.path().display()))).unwrap();
    assert_eq!(sc.face_regions().len(), 1);
}

#[test]
fn rebuilding_twice_does_not_duplicate_restored_faces() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("catalog.db");
    let (cat, _file_id, _person) = seed_named(dir.path());
    drop(cat);
    for suffix in ["", "-wal", "-shm"] {
        // Missing -wal/-shm files are normal; only the database itself
        // must exist.
        let mut p = db.clone().into_os_string();
        p.push(suffix);
        let _ = std::fs::remove_file(&p);
    }
    let mut cat = open(dir.path());
    add_photo(&mut cat, dir.path(), "p0.jpg");
    cat.rebuild_from_sidecars(true).unwrap();
    cat.rebuild_from_sidecars(true).unwrap();

    let faces = cat.faces_for_file(1).unwrap();
    assert_eq!(faces.len(), 1, "no duplicates across rebuilds: {faces:?}");
    let persons: i64 =
        cat.conn().query_row("SELECT COUNT(*) FROM persons", [], |r| r.get(0)).unwrap();
    assert_eq!(persons, 1);
}

#[test]
fn a_region_removed_from_the_sidecar_unnames_the_face() {
    let dir = tempfile::tempdir().unwrap();
    let (mut cat, file_id, _person) = seed_named(dir.path());

    // Another tool (digiKam) rewrites the sidecar without the face region.
    let sc_path = sidecar_path(&format!("{}/p0.jpg", dir.path().display()));
    let scratch = dir.path().join("scratch.xmp");
    let mut fresh = Sidecar::read(&scratch).unwrap();
    fresh.set_rating(4);
    fresh.write(&scratch).unwrap();
    std::fs::copy(&scratch, &sc_path).unwrap();

    // Sidecar wins on the next convergence (§5 conflict rule).
    assert!(cat.sync_file(file_id).unwrap());
    let faces = cat.faces_for_file(file_id).unwrap();
    assert_eq!(faces.len(), 1, "the detection itself is derived data, it stays");
    let named: Option<i64> = cat
        .conn()
        .query_row("SELECT person_id FROM faces WHERE id = ?1", [faces[0].id], |r| r.get(0))
        .unwrap();
    assert_eq!(named, None, "{faces:?}");
    assert!(cat.search_files("Anna", 100).unwrap().is_empty());
}
