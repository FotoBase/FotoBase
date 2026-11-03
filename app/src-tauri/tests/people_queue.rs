//! TASK-05-005 integration: the unnamed people queue's app half. The
//! catalog owns the rules (crates/catalog/tests/people.rs); here the IPC
//! payloads and the bulk-action routing are checked against a real seeded
//! catalog — queue serialization, confirm touching every face, reject
//! touching none of the confirmed data.

use fotobase_catalog::{sidecar_path, Catalog, FaceRecord};
use fotobase_lib::people::{self, BulkAction};
use fotobase_metadata::Sidecar;

fn add_photo(cat: &mut Catalog, dir: &std::path::Path, name: &str) -> i64 {
    let original = dir.join(name);
    std::fs::write(&original, format!("fake image bytes of {name}")).unwrap();
    let folder_id = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
            rusqlite::params![folder_id, original.to_str().unwrap(), name],
        )
        .unwrap();
    cat.conn().last_insert_rowid()
}

fn seed_cluster(cat: &mut Catalog, dir: &std::path::Path, center: [f32; 8], n: usize) -> Vec<i64> {
    let mut s = 7u64;
    let mut rnd = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s as f32 / u64::MAX as f32 - 0.5
    };
    (0..n)
        .map(|i| {
            let mut v = center;
            for c in v.iter_mut() {
                *c += rnd() * 0.01;
            }
            let file_id = add_photo(cat, dir, &format!("p{i}.jpg"));
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
fn queue_serializes_and_bulk_confirm_routes_through() {
    let (lib, _db, mut cat) = fixture();
    let ids = seed_cluster(&mut cat, lib.path(), [-1.0; 8], 25);
    cat.cluster_faces(5).unwrap();

    let queue = people::queue(&cat).unwrap();
    assert_eq!(queue.len(), 1);
    let entry = &queue[0];
    let clustered: Vec<i64> = cat.face_cluster_assignments(&ids).unwrap().keys().copied().collect();
    assert_eq!(entry.face_count as usize, clustered.len());
    assert_eq!(entry.face_id, *clustered.iter().min().unwrap(), "lowest face id is the rep");
    assert_eq!(entry.hash, None, "no content hash in this fixture");

    let cluster_id = entry.cluster_id;
    let confirmed = people::bulk(
        &mut cat,
        cluster_id,
        BulkAction::Confirm(people::ConfirmArg { name: "Alice".into() }),
    )
    .unwrap();
    assert_eq!(confirmed, clustered.len());

    // Every face's photo carries the named MWG region (§5 user data).
    for id in &clustered {
        let file_id: i64 = cat
            .conn()
            .query_row("SELECT file_id FROM faces WHERE id = ?1", [id], |r| r.get(0))
            .unwrap();
        let original: String = cat
            .conn()
            .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))
            .unwrap();
        let sc = Sidecar::read(&sidecar_path(&original)).unwrap();
        let regions = sc.face_regions();
        assert_eq!(regions.len(), 1, "face {id}");
        assert_eq!(regions[0].name, "Alice");
    }
    assert!(people::queue(&cat).unwrap().is_empty());
}

#[test]
fn queue_bulk_reject_routes_through_and_clears_the_queue() {
    let (lib, _db, mut cat) = fixture();
    let ids = seed_cluster(&mut cat, lib.path(), [-1.0; 8], 25);
    cat.cluster_faces(5).unwrap();
    let cluster_id = people::queue(&cat).unwrap()[0].cluster_id;

    let clustered: Vec<i64> = cat.face_cluster_assignments(&ids).unwrap().keys().copied().collect();
    let rejected = people::bulk(&mut cat, cluster_id, BulkAction::Reject).unwrap();
    assert_eq!(rejected, clustered.len());
    assert!(people::queue(&cat).unwrap().is_empty());

    // The faces stay detected: rows remain, no person attached, no sidecar.
    for id in &clustered {
        let file_id: i64 = cat
            .conn()
            .query_row("SELECT file_id FROM faces WHERE id = ?1", [id], |r| r.get(0))
            .unwrap();
        let person_id: Option<i64> = cat
            .conn()
            .query_row("SELECT person_id FROM faces WHERE id = ?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(person_id, None);
        let original: String = cat
            .conn()
            .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))
            .unwrap();
        assert!(!sidecar_path(&original).exists(), "reject must not write sidecars");
    }
    assert!(cat.face_person(ids[0]).unwrap().is_none());
}
