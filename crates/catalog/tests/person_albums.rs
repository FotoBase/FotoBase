//! TASK-05-007: person albums, the "has faces" filter and person names in
//! FTS (PLAN §2G "Person albums", §2C filter buttons, §5 `files_fts` people
//! column). Auto albums are derived catalog rows (`albums.person_id`) — never
//! written to a sidecar's `fb:Albums` or to `albums.json`.

use fotobase_catalog::{sidecar_path, Catalog, FaceRecord, FileQuery};
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

fn face() -> FaceRecord {
    FaceRecord {
        x: 0.1,
        y: 0.2,
        width: 0.2,
        height: 0.25,
        landmarks: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
        confidence: 0.9,
    }
}

fn blob(center: [f32; 128], n: usize, seed: u64) -> Vec<[f32; 128]> {
    let mut s = seed | 1;
    let mut rnd = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s as f32 / u64::MAX as f32 - 0.5
    };
    (0..n)
        .map(|_| {
            let mut v = center;
            // Vary only the first 8 components: uniform noise across all
            // 128 dims concentrates pairwise distances and fragments the
            // synthetic clusters; real (unit-norm) embeddings live on a much
            // lower-dimensional manifold.
            for c in v.iter_mut().take(8) {
                *c += rnd() * 0.01;
            }
            v
        })
        .collect()
}

const A: [f32; 128] = [-1.0; 128];
const B: [f32; 128] = [1.0; 128];

struct Seeded {
    _dir: tempfile::TempDir,
    cat: Catalog,
    /// `(file_id, face_id)` of cluster A's ten photos.
    a: Vec<(i64, i64)>,
    /// Same for cluster B.
    b: Vec<(i64, i64)>,
    cluster_a: i64,
}

fn seed() -> Seeded {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let mut embs = blob(A, 10, 1);
    embs.extend(blob(B, 10, 2));
    let rows: Vec<(i64, i64)> = embs
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let file_id = add_photo(&mut cat, dir.path(), &format!("p{i}.jpg"));
            (file_id, cat.record_face(file_id, &face(), Some(v)).unwrap())
        })
        .collect();
    cat.cluster_faces(5).unwrap();
    let face_ids: Vec<i64> = rows.iter().map(|r| r.1).collect();
    let cluster_a = cat.face_cluster_assignments(&face_ids[..10]).unwrap()[&face_ids[0]];
    let (a, b) = (rows[..10].to_vec(), rows[10..].to_vec());
    Seeded { _dir: dir, cat, a, b, cluster_a }
}

fn named_photos(cat: &Catalog, person_id: i64) -> Vec<i64> {
    let mut stmt = cat
        .conn()
        .prepare("SELECT DISTINCT file_id FROM faces WHERE person_id = ?1 ORDER BY file_id")
        .unwrap();
    stmt.query_map([person_id], |r| r.get(0)).unwrap().flatten().collect()
}

fn person_album(cat: &Catalog, name: &str) -> Option<fotobase_catalog::AlbumInfo> {
    cat.list_albums().unwrap().into_iter().find(|a| a.person_id.is_some() && a.name == name)
}

fn sorted(mut v: Vec<i64>) -> Vec<i64> {
    v.sort_unstable();
    v
}

#[test]
fn naming_a_person_creates_an_album_of_their_photos_and_fts_finds_them() {
    let mut s = seed();
    let person = s.cat.name_cluster(s.cluster_a, "Anna").unwrap();

    let album = person_album(&s.cat, "Anna").expect("auto album for Anna");
    assert_eq!(album.person_id, Some(person.id));
    let photos = named_photos(&s.cat, person.id);
    assert!(!photos.is_empty());
    assert_eq!(album.count, photos.len() as i64);
    assert_eq!(sorted(s.cat.album_file_ids(album.id).unwrap()), photos);

    assert_eq!(sorted(s.cat.search_files("Anna", 100).unwrap()), photos, "FTS finds Anna");
    assert_eq!(sorted(s.cat.search_files("ann", 100).unwrap()), photos, "prefix search");
    // The combined query pipeline finds them through the same index.
    let q = FileQuery { text: Some("Anna".into()), ..FileQuery::default() };
    let hit_ids: Vec<i64> = s.cat.query_files(&q, 100).unwrap().iter().map(|f| f.id).collect();
    assert_eq!(sorted(hit_ids), photos);
    // Unnamed cluster B photos are not found by her name.
    for (file_id, _) in &s.b {
        assert!(!s.cat.search_files("Anna", 100).unwrap().contains(file_id));
    }
}

#[test]
fn confirming_a_suggestion_adds_the_photo_to_the_album_and_fts() {
    let mut s = seed();
    let person = s.cat.name_cluster(s.cluster_a, "Anna").unwrap();
    let before = named_photos(&s.cat, person.id).len();

    let (file_id, face_id) = s.b[0];
    s.cat.confirm_suggestion(face_id, person.id).unwrap();

    let album = person_album(&s.cat, "Anna").unwrap();
    assert_eq!(album.count, before as i64 + 1);
    assert!(s.cat.album_file_ids(album.id).unwrap().contains(&file_id));
    assert!(s.cat.search_files("Anna", 100).unwrap().contains(&file_id));
}

#[test]
fn bulk_confirm_creates_the_album() {
    let mut s = seed();
    let cluster_b = s.cat.face_cluster_assignments(&[s.b[0].1]).unwrap()[&s.b[0].1];
    let (person, confirmed) = s.cat.bulk_confirm_cluster(cluster_b, "Ben").unwrap();
    assert!(confirmed > 0);
    let album = person_album(&s.cat, "Ben").unwrap();
    assert_eq!(album.person_id, Some(person.id));
    assert_eq!(album.count, confirmed as i64);
    assert_eq!(s.cat.search_files("Ben", 100).unwrap().len(), confirmed);
}

#[test]
fn renaming_a_person_renames_the_album_and_fts_follows() {
    let mut s = seed();
    let person = s.cat.name_cluster(s.cluster_a, "Anna").unwrap();
    let photos = named_photos(&s.cat, person.id);

    s.cat.rename_person(person.id, "Hannah").unwrap();

    assert!(person_album(&s.cat, "Anna").is_none(), "old album name left behind");
    let album = person_album(&s.cat, "Hannah").expect("album follows the rename");
    assert_eq!(album.person_id, Some(person.id));
    assert_eq!(sorted(s.cat.album_file_ids(album.id).unwrap()), photos);
    assert_eq!(sorted(s.cat.search_files("Hannah", 100).unwrap()), photos);
    assert!(s.cat.search_files("Anna", 100).unwrap().is_empty(), "old name still searchable");
}

#[test]
fn has_faces_filter_matches_only_photos_with_face_rows() {
    let mut s = seed();
    let plain = add_photo(&mut s.cat, s._dir.path(), "no-faces.jpg");
    let all: Vec<i64> =
        s.cat.query_files(&FileQuery::default(), 100).unwrap().iter().map(|f| f.id).collect();
    assert!(all.contains(&plain));

    let q = FileQuery { has_faces: true, ..FileQuery::default() };
    let hits: Vec<i64> = s.cat.query_files(&q, 100).unwrap().iter().map(|f| f.id).collect();
    let expected = sorted(s.a.iter().chain(&s.b).map(|r| r.0).collect());
    assert_eq!(sorted(hits), expected);
    assert!(!expected.contains(&plain));

    // ANDs with the other filters: has faces + search text for a name.
    s.cat.name_cluster(s.cluster_a, "Anna").unwrap();
    let q = FileQuery { has_faces: true, text: Some("Anna".into()), ..FileQuery::default() };
    let hits = s.cat.query_files(&q, 100).unwrap();
    assert!(!hits.is_empty() && hits.iter().all(|f| s.a.iter().any(|r| r.0 == f.id)));
}

#[test]
fn auto_albums_never_reach_sidecars_or_albums_json() {
    let mut s = seed();
    let person = s.cat.name_cluster(s.cluster_a, "Anna").unwrap();
    let album = person_album(&s.cat, "Anna").unwrap();

    // An unrelated user-data write re-mirrors the photo from its sidecar;
    // the derived membership must survive it.
    let (file_id, _) = s.a[0];
    s.cat.set_rating(file_id, 5).unwrap();
    assert!(s.cat.album_file_ids(album.id).unwrap().contains(&file_id));
    assert!(s.cat.file_album_ids(file_id).unwrap().contains(&album.id));

    let path: String = s
        .cat
        .conn()
        .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))
        .unwrap();
    let sc = Sidecar::read(&sidecar_path(&path)).unwrap();
    assert!(sc.albums().is_empty(), "auto album id leaked into fb:Albums: {:?}", sc.albums());

    s.cat.create_album("Trip", "2026-01-01", None, None).unwrap();
    let json = std::fs::read_to_string(s.cat.albums_json_path()).unwrap();
    assert!(!json.contains("Anna"), "auto album persisted to albums.json: {json}");

    // The album is derived: user edits are refused, nothing changes.
    assert!(s.cat.update_album(album.id, "Other", "2026-01-01", None, None).is_err());
    assert!(s.cat.delete_album(album.id).is_err());
    assert!(s.cat.add_to_album(album.id, &[s.b[0].0]).is_err());
    assert!(s.cat.remove_from_album(album.id, &[file_id]).is_err());
    assert_eq!(person_album(&s.cat, "Anna").unwrap().count, album.count);
    assert_eq!(person_album(&s.cat, "Anna").unwrap().person_id, Some(person.id));
}

#[test]
fn auto_albums_survive_reopening_the_catalog() {
    let mut s = seed();
    s.cat.create_album("Trip", "2026-01-01", None, None).unwrap();
    s.cat.name_cluster(s.cluster_a, "Anna").unwrap();
    let count = person_album(&s.cat, "Anna").unwrap().count;
    drop(s.cat);

    let cat = open(s._dir.path());
    assert_eq!(person_album(&cat, "Anna").unwrap().count, count);
}

#[test]
fn removing_the_last_face_of_a_person_in_a_photo_drops_it_from_the_album() {
    let mut s = seed();
    let person = s.cat.name_cluster(s.cluster_a, "Anna").unwrap();
    let (file_id, face_id) = s.a[0];

    s.cat.conn().execute("DELETE FROM faces WHERE id = ?1", [face_id]).unwrap();

    let album = person_album(&s.cat, "Anna").unwrap();
    assert!(!s.cat.album_file_ids(album.id).unwrap().contains(&file_id));
    assert!(!s.cat.search_files("Anna", 100).unwrap().contains(&file_id));
    assert_eq!(album.count, named_photos(&s.cat, person.id).len() as i64);
}

#[test]
fn deleting_a_photo_cleans_album_and_fts() {
    let mut s = seed();
    s.cat.name_cluster(s.cluster_a, "Anna").unwrap();
    let (file_id, _) = s.a[0];
    s.cat.conn().execute("DELETE FROM files WHERE id = ?1", [file_id]).unwrap();

    assert!(!s.cat.search_files("Anna", 100).unwrap().contains(&file_id));
    let fts_rows: i64 = s
        .cat
        .conn()
        .query_row("SELECT COUNT(*) FROM files_fts WHERE rowid = ?1", [file_id], |r| r.get(0))
        .unwrap();
    assert_eq!(fts_rows, 0);
}

#[test]
fn portable_export_skips_derived_person_albums() {
    let mut s = seed();
    s.cat.create_album("Trip", "2026-01-01", None, None).unwrap();
    s.cat.name_cluster(s.cluster_a, "Anna").unwrap();

    let sections = fotobase_catalog::ExportSections { albums: true, tags: false, recipes: false };
    let albums = s.cat.portable_export(sections).unwrap().albums.unwrap();
    let names: Vec<&str> = albums.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Trip"]);
}

/// A catalog already holding named faces (schema v16) gets its albums and
/// FTS people column backfilled by the migration.
#[test]
fn migration_backfills_person_albums_and_fts_people() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &fotobase_catalog::MIGRATIONS[..16] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute_batch("PRAGMA user_version = 16").unwrap();
        conn.execute("INSERT INTO folders(path, scan_mode) VALUES ('/p', 'once')", []).unwrap();
        conn.execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, media_type)
             VALUES (1, '/p/a.jpg', 'a.jpg', 1, 0.0, 'image')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO persons(name) VALUES ('Anna')", []).unwrap();
        conn.execute(
            "INSERT INTO faces(file_id, x, y, width, height, confidence, person_id)
             VALUES (1, 0.1, 0.1, 0.2, 0.2, 0.9, 1)",
            [],
        )
        .unwrap();
    }
    let cat = Catalog::open(&path).unwrap();
    let album = person_album(&cat, "Anna").expect("backfilled album");
    assert_eq!(album.count, 1);
    assert_eq!(cat.search_files("Anna", 10).unwrap(), vec![1]);
}
