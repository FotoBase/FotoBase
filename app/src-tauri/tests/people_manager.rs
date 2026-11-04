//! TASK-05-006 integration: the People Manager's app half. The catalog owns
//! the rules (crates/catalog/tests/people.rs); here the IPC payloads and the
//! rename routing are checked against a real seeded catalog — entries list
//! named + unnamed, rename rewriting every confirmed face's sidecar regions.

use fotobase_catalog::{sidecar_path, Catalog, FaceRecord};
use fotobase_lib::people;
use fotobase_metadata::Sidecar;

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

fn seed_cluster(
    cat: &mut Catalog,
    dir: &std::path::Path,
    center: [f32; 128],
    n: usize,
) -> Vec<i64> {
    let mut s = 7u64;
    let mut rnd = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s as f32 / u64::MAX as f32 - 0.5
    };
    let offset: usize =
        cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap();
    (0..n)
        .map(|i| {
            let mut v = center;
            for c in v.iter_mut() {
                *c += rnd() * 0.01;
            }
            let file_id = add_photo(cat, dir, &format!("p{}.jpg", offset + i));
            cat.record_face(
                file_id,
                &FaceRecord {
                    x: 0.1,
                    y: 0.2,
                    width: 0.2,
                    height: 0.25,
                    landmarks: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
                    confidence: 0.9,
                },
                Some(&v),
            )
            .unwrap()
        })
        .collect()
}

fn fixture() -> (tempfile::TempDir, tempfile::TempDir, Catalog) {
    let lib = tempfile::tempdir().unwrap();
    let db = tempfile::tempdir().unwrap();
    let cat = Catalog::open(db.path().join("catalog.db")).unwrap();
    (lib, db, cat)
}

#[test]
fn entries_list_named_and_unnamed_and_rename_routes_through() {
    let (lib, _db, mut cat) = fixture();
    let a = seed_cluster(&mut cat, lib.path(), [-1.0; 128], 5);
    let b = seed_cluster(&mut cat, lib.path(), [1.0; 128], 5);
    cat.cluster_faces(5).unwrap();

    // Nothing named yet: one unnamed cluster per group.
    let entries = people::entries(&cat).unwrap();
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert!(entries.iter().all(|e| e.person_id.is_none()));

    let ca = cat.face_cluster_assignments(&[a[0]]).unwrap()[&a[0]];
    let cb = cat.face_cluster_assignments(&[b[0]]).unwrap()[&b[0]];
    let (alice, _) = cat.bulk_confirm_cluster(ca, "Alice").unwrap();
    cat.bulk_confirm_cluster(cb, "Bob").unwrap();

    let entries = people::entries(&cat).unwrap();
    assert_eq!(entries.len(), 2);
    let named = entries.iter().find(|e| e.person_id == Some(alice.id)).unwrap();
    assert_eq!(named.name.as_deref(), Some("Alice"));
    assert_eq!(named.face_count, 5);
    assert_eq!(named.representative.as_ref().unwrap().face_id, *a.iter().min().unwrap());

    // Rename routes through: every Alice sidecar is rewritten.
    let renamed = people::rename(&mut cat, alice.id, "Alicia").unwrap();
    assert_eq!(renamed.name, "Alicia");
    for id in &a {
        let file_id: i64 = cat
            .conn()
            .query_row("SELECT file_id FROM faces WHERE id = ?1", [id], |r| r.get(0))
            .unwrap();
        let original: String = cat
            .conn()
            .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))
            .unwrap();
        let regions = Sidecar::read(&sidecar_path(&original)).unwrap().face_regions();
        assert_eq!(regions[0].name, "Alicia", "face {id}");
    }
    // Bob untouched.
    let file_id: i64 = cat
        .conn()
        .query_row("SELECT file_id FROM faces WHERE id = ?1", [b[0]], |r| r.get(0))
        .unwrap();
    let original: String = cat
        .conn()
        .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))
        .unwrap();
    assert_eq!(Sidecar::read(&sidecar_path(&original)).unwrap().face_regions()[0].name, "Bob");
}
