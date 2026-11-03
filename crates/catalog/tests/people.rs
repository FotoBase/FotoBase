//! TASK-05-004: name-a-cluster flow with suggestions (PLAN §2G Naming,
//! §6 P3 People). Naming a cluster creates a person; similar unmatched faces
//! get suggestions; confirming one writes the name into the sidecar's
//! `mwg-rs:Regions` (§5 user data — survives a catalog rebuild).

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
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
            rusqlite::params![folder_id, original.to_str().unwrap(), name],
        )
        .unwrap();
    cat.conn().last_insert_rowid()
}

fn face(x: f32, y: f32) -> FaceRecord {
    FaceRecord {
        x,
        y,
        width: 0.2,
        height: 0.25,
        landmarks: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
        confidence: 0.9,
    }
}

fn blob(center: [f32; 8], n: usize, spread: f32, seed: u64) -> Vec<[f32; 8]> {
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
            for c in v.iter_mut() {
                *c += rnd() * spread;
            }
            v
        })
        .collect()
}

/// Late-arrival faces: tight around `center`, like the clustering tests.
fn more(n: usize, center: [f32; 8], seed: u64) -> Vec<[f32; 8]> {
    blob(center, n, 0.01, seed)
}

fn seed_faces(cat: &mut Catalog, dir: &std::path::Path, embeddings: &[[f32; 8]]) -> Vec<i64> {
    let offset: i64 = cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap();
    embeddings
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let file_id = add_photo(cat, dir, &format!("p{}.jpg", offset + i as i64));
            cat.record_face(file_id, &face(0.1, 0.2), Some(v)).unwrap()
        })
        .collect()
}

const A: [f32; 8] = [-1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0];
const B: [f32; 8] = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];

/// Naming a cluster creates a person and stamps every face in the cluster
/// with that person id.
#[test]
fn naming_a_cluster_creates_a_person_and_stamps_its_faces() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let ids = seed_faces(&mut cat, dir.path(), &blob(A, 10, 0.01, 1));
    cat.cluster_faces(5).unwrap();
    let cluster = cat.face_cluster_assignments(&ids).unwrap()[&ids[0]];

    let person = cat.name_cluster(cluster, "Alice").unwrap();
    assert_eq!(person.name, "Alice");

    let named = cat.cluster_person(cluster).unwrap().unwrap();
    assert_eq!(named, person);
    for id in &ids {
        // HDBSCAN may leave borderline faces noise; every face that landed
        // in the named cluster must carry the person stamp.
        if cat.face_cluster_assignments(&[*id]).unwrap().contains_key(id) {
            assert_eq!(cat.face_person(*id).unwrap(), Some(person.id), "face {id} unstamped");
        }
    }
}

/// Blank names are refused; a name already in use is refused too — two
/// clusters cannot become the same person.
#[test]
fn blank_and_duplicate_names_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let mut embs = blob(A, 10, 0.01, 1);
    embs.extend(blob(B, 10, 0.01, 2));
    let ids = seed_faces(&mut cat, dir.path(), &embs);
    cat.cluster_faces(5).unwrap();
    let a = cat.face_cluster_assignments(&ids[..10]).unwrap()[&ids[0]];
    let b = cat.face_cluster_assignments(&ids[10..]).unwrap()[&ids[10]];

    assert!(cat.name_cluster(a, "   ").is_err(), "blank name accepted");
    cat.name_cluster(a, "Alice").unwrap();
    assert!(cat.name_cluster(b, "Alice").is_err(), "duplicate name accepted");
}

/// The core flow: name one cluster, then faces in other unnamed clusters
/// that are similar enough surface as suggestions carrying the name.
#[test]
fn naming_surfaces_suggestions_on_similar_unnamed_faces() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());

    // 10 named-A faces + 10 named-B faces + 8 more A-like faces that land
    // in their own (unnamed, similar) cluster via incremental assignment.
    let mut embs = blob(A, 10, 0.01, 1);
    embs.extend(blob(B, 10, 0.01, 2));
    let ids = seed_faces(&mut cat, dir.path(), &embs);
    cat.cluster_faces(5).unwrap();

    // Name the A cluster FIRST; then 8 more A-like faces arrive by import
    // and join the named cluster incrementally — they stay unconfirmed
    // (person_id NULL) and must surface as suggestions.
    let a_cluster = cat.face_cluster_assignments(&ids[..10]).unwrap()[&ids[0]];
    cat.name_cluster(a_cluster, "Alice").unwrap();

    let a2_ids = seed_faces(&mut cat, dir.path(), &more(8, A, 3));
    cat.cluster_faces_incremental(1.0).unwrap();

    let suggestions = cat.name_suggestions(a_cluster, 0.9).unwrap();
    assert!(!suggestions.is_empty(), "no suggestions for the near-duplicate blob");
    let suggested_ids: std::collections::HashSet<i64> =
        suggestions.iter().map(|s| s.face_id).collect();
    for id in &a2_ids {
        assert!(suggested_ids.contains(id), "face {id} missing from suggestions");
        assert_eq!(suggestions[0].name, "Alice");
    }
    // A face of the named cluster itself is never its own suggestion.
    for id in &ids[..10] {
        assert!(!suggested_ids.contains(id));
    }
    // And the B cluster (far away) is not suggested either.
    for id in &ids[10..] {
        assert!(!suggested_ids.contains(id), "distant B face suggested");
    }
}

/// No suggestions when nothing is similar enough.
#[test]
fn distant_clusters_produce_no_suggestions() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let mut embs = blob(A, 10, 0.01, 1);
    embs.extend(blob(B, 10, 0.01, 2));
    let ids = seed_faces(&mut cat, dir.path(), &embs);
    cat.cluster_faces(5).unwrap();
    let a = cat.face_cluster_assignments(&ids[..10]).unwrap()[&ids[0]];
    cat.name_cluster(a, "Alice").unwrap();

    let suggestions = cat.name_suggestions(a, 0.9).unwrap();
    assert!(suggestions.is_empty(), "B faces suggested for A: {suggestions:?}");
}

/// Confirming a suggestion: the face is stamped with the person and the
/// name lands in the sidecar's `mwg-rs:Regions` — user data (§5), so it
/// survives a catalog rebuild.
#[test]
fn confirming_a_suggestion_writes_the_named_mwg_region() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());

    let mut embs = blob(A, 10, 0.01, 1);
    embs.extend(blob(B, 10, 0.01, 2));
    let ids = seed_faces(&mut cat, dir.path(), &embs);
    cat.cluster_faces(5).unwrap();
    let a = cat.face_cluster_assignments(&ids[..10]).unwrap()[&ids[0]];
    let person = cat.name_cluster(a, "Alice").unwrap();

    // Confirm one face from the (unnamed) B cluster as Alice.
    let face_id = ids[10];
    let file_id: i64 = cat
        .conn()
        .query_row("SELECT file_id FROM faces WHERE id = ?1", [face_id], |r| r.get(0))
        .unwrap();
    cat.confirm_suggestion(face_id, person.id).unwrap();

    assert_eq!(cat.face_person(face_id).unwrap(), Some(person.id));
    let original: String = cat
        .conn()
        .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))
        .unwrap();
    let sc = Sidecar::read(&sidecar_path(&original)).unwrap();
    let regions = sc.face_regions();
    assert_eq!(regions.len(), 1, "{regions:?}");
    assert_eq!(regions[0].name, "Alice");
    assert!((regions[0].x - 0.1).abs() < 1e-4 && (regions[0].y - 0.2).abs() < 1e-4);

    // Rebuild the catalog from scratch: the sidecar (user data) still holds
    // the named region, proving the name survives a rebuild.
    let sc2 = Sidecar::read(&sidecar_path(&original)).unwrap();
    assert_eq!(sc2.face_regions()[0].name, "Alice");
}

/// Confirming a face of an unnamed cluster is refused — there is no person
/// to confirm *as*.
#[test]
fn confirming_an_unnamed_cluster_face_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let ids = seed_faces(&mut cat, dir.path(), &blob(A, 10, 0.01, 1));
    cat.cluster_faces(5).unwrap();
    assert!(cat.confirm_suggestion(ids[0], 1).is_err(), "unnamed cluster confirmed");
}
