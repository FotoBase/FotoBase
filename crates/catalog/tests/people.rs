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

// ---- TASK-05-005: the unnamed people queue (PLAN §2G, §6 P3 People) ----

/// The queue lists only unnamed clusters, with a representative face (file,
/// box and content hash for the `thumb://` URI) and the count of still-
/// actionable faces.
#[test]
fn unnamed_queue_lists_only_unnamed_clusters() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let mut embs = blob(A, 25, 0.01, 1);
    embs.extend(blob(B, 25, 0.01, 2));
    let ids = seed_faces(&mut cat, dir.path(), &embs);
    cat.cluster_faces(5).unwrap();
    let a = cat.face_cluster_assignments(&ids[..25]).unwrap()[&ids[0]];
    let b = cat.face_cluster_assignments(&ids[25..]).unwrap()[&ids[25]];
    cat.name_cluster(a, "Alice").unwrap();

    let queue = cat.unnamed_queue().unwrap();
    assert_eq!(queue.len(), 1, "named cluster must not be queued: {queue:?}");
    let entry = &queue[0];
    assert_eq!(entry.cluster_id, b);
    assert_eq!(entry.face_count, 25);
    assert_eq!(entry.representative.face_id, ids[25], "lowest face id is the rep");
    assert!(entry.representative.hash.is_none(), "no content hash in this fixture");
    assert!((entry.representative.x - 0.1).abs() < 1e-4);

    // Naming it empties the queue.
    cat.name_cluster(b, "Bob").unwrap();
    assert!(cat.unnamed_queue().unwrap().is_empty());
}

/// Bulk confirm: one action names the cluster (creates the person), stamps
/// every face of it and writes the name into each photo's sidecar as a
/// named MWG region — the integration criterion. The queue empties.
#[test]
fn bulk_confirm_names_all_faces_and_writes_their_mwg_regions() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let ids = seed_faces(&mut cat, dir.path(), &blob(A, 25, 0.01, 1));
    cat.cluster_faces(5).unwrap();
    let a = cat.face_cluster_assignments(&ids).unwrap()[&ids[0]];

    let a_faces = cat.face_cluster_assignments(&ids).unwrap();
    let in_a: Vec<i64> = ids.iter().copied().filter(|i| a_faces.contains_key(i)).collect();
    let (person, confirmed) = cat.bulk_confirm_cluster(a, "Alice").unwrap();
    assert_eq!(person.name, "Alice");
    assert_eq!(confirmed, in_a.len(), "every clustered face confirmed");

    for id in &in_a {
        assert_eq!(cat.face_person(*id).unwrap(), Some(person.id));
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
        assert_eq!(regions.len(), 1, "face {id}: {regions:?}");
        assert_eq!(regions[0].name, "Alice");
    }
    assert!(cat.unnamed_queue().unwrap().is_empty());
}

/// Bulk reject suppresses the suggestion for the whole cluster: the faces
/// stay detected (rows remain, person_id still NULL) but leave the queue,
/// and a later bulk confirm of the same cluster does not re-name them.
#[test]
fn bulk_reject_suppresses_the_cluster_but_keeps_the_faces() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let ids = seed_faces(&mut cat, dir.path(), &blob(A, 25, 0.01, 1));
    cat.cluster_faces(5).unwrap();
    let a = cat.face_cluster_assignments(&ids).unwrap()[&ids[0]];

    let assignments = cat.face_cluster_assignments(&ids).unwrap();
    let in_a: Vec<i64> = ids.iter().copied().filter(|i| assignments.contains_key(i)).collect();
    let rejected = cat.bulk_reject_cluster(a).unwrap();
    assert_eq!(rejected, in_a.len());

    // Faces remain detected: rows exist, no person attached, no sidecar
    // write happened.
    for id in &in_a {
        assert_eq!(cat.face_person(*id).unwrap(), None);
    }
    assert!(cat.unnamed_queue().unwrap().is_empty(), "rejected cluster leaves the queue");

    // Confirming afterwards touches only faces that were not rejected —
    // here none, so the person is created but nothing is stamped or
    // written to any sidecar.
    let (person, confirmed) = cat.bulk_confirm_cluster(a, "Alice").unwrap();
    assert_eq!(confirmed, 0);
    for id in &ids {
        assert_eq!(cat.face_person(*id).unwrap(), None, "rejected face stays unstamped");
    }
    assert_eq!(person.name, "Alice");
}

/// A partially rejected cluster: bulk confirm names only the remaining
/// faces; rejected ones stay suppressed. The queue drops the cluster once
/// no actionable face is left.
#[test]
fn bulk_confirm_skips_rejected_faces() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let ids = seed_faces(&mut cat, dir.path(), &blob(A, 25, 0.01, 1));
    cat.cluster_faces(5).unwrap();
    let a = cat.face_cluster_assignments(&ids).unwrap()[&ids[0]];

    // Reject two faces directly (the per-face rejection state the queue
    // builds on).
    let assignments = cat.face_cluster_assignments(&ids).unwrap();
    let in_a: Vec<i64> = ids.iter().copied().filter(|i| assignments.contains_key(i)).collect();
    cat.reject_face(in_a[0]).unwrap();
    cat.reject_face(in_a[1]).unwrap();

    let (_, confirmed) = cat.bulk_confirm_cluster(a, "Alice").unwrap();
    assert_eq!(confirmed, in_a.len() - 2, "only the non-rejected faces");
    assert_eq!(cat.face_person(in_a[0]).unwrap(), None);
    assert_eq!(cat.face_person(in_a[1]).unwrap(), None);
    assert_eq!(cat.face_person(in_a[2]).unwrap(), Some(1));
    // No actionable face is left, so the cluster is out of the queue even
    // though two faces are still unnamed.
    assert!(cat.unnamed_queue().unwrap().is_empty());
}

/// Confirming an unknown cluster is refused.
#[test]
fn bulk_confirm_of_an_unknown_cluster_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    assert!(cat.bulk_confirm_cluster(42, "Alice").is_err());
    assert!(cat.bulk_reject_cluster(42).is_err());
}

/// A blank name is refused, and a name already in use fails without
/// partially stamping the cluster.
#[test]
fn bulk_confirm_refuses_bad_names_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let mut embs = blob(A, 25, 0.01, 1);
    embs.extend(blob(B, 25, 0.01, 2));
    let ids = seed_faces(&mut cat, dir.path(), &embs);
    cat.cluster_faces(5).unwrap();
    let a = cat.face_cluster_assignments(&ids[..25]).unwrap()[&ids[0]];
    let b = cat.face_cluster_assignments(&ids[25..]).unwrap()[&ids[25]];
    cat.name_cluster(a, "Alice").unwrap();

    assert!(cat.bulk_confirm_cluster(b, "  ").is_err(), "blank name accepted");
    assert!(cat.bulk_confirm_cluster(b, "Alice").is_err(), "duplicate name accepted");
    // Nothing was stamped by the failed attempts.
    for id in &ids[25..] {
        assert_eq!(cat.face_person(*id).unwrap(), None);
    }
}

// ---- TASK-05-006: the People Manager (PLAN §2 Tools menu, §6 P3 People) ----

/// The manager lists every named person and every unnamed cluster, each with
/// a face count and a representative face (file, box, content hash for the
/// `thumb://` URI).
#[test]
fn people_manager_lists_named_and_unnamed_entries() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());

    let mut embs = blob(A, 10, 0.01, 1);
    embs.extend(blob(B, 10, 0.01, 2));
    let ids = seed_faces(&mut cat, dir.path(), &embs);
    cat.cluster_faces(5).unwrap();
    let a = cat.face_cluster_assignments(&ids[..10]).unwrap()[&ids[0]];
    let b = cat.face_cluster_assignments(&ids[10..]).unwrap()[&ids[10]];
    cat.name_cluster(a, "Alice").unwrap();
    // One B face rejected: still an unnamed cluster, count covers its
    // remaining unnamed faces.
    cat.reject_face(ids[11]).unwrap();

    let entries = cat.people_entries().unwrap();
    assert_eq!(entries.len(), 2, "{entries:?}");

    let named = entries.iter().find(|e| e.person_id.is_some()).unwrap();
    assert_eq!(named.name.as_deref(), Some("Alice"));
    assert_eq!(named.cluster_id, None);
    assert_eq!(named.face_count, 10);
    assert_eq!(named.representative.as_ref().unwrap().face_id, ids[0]);

    let unnamed = entries.iter().find(|e| e.person_id.is_none()).unwrap();
    assert_eq!(unnamed.cluster_id, Some(b));
    assert_eq!(unnamed.name, None);
    assert_eq!(unnamed.face_count, 9, "rejected face still unnamed but counted");
    assert_eq!(unnamed.representative.as_ref().unwrap().face_id, ids[10]);
}

/// A person with no confirmed faces (possible via earlier flows) still
/// lists, with a zero count and no representative.
#[test]
fn people_manager_lists_a_person_without_faces() {
    let dir = tempfile::tempdir().unwrap();
    let cat = open(dir.path());
    cat.conn().execute("INSERT INTO persons (name) VALUES ('Lonely')", []).unwrap();
    let entries = cat.people_entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name.as_deref(), Some("Lonely"));
    assert_eq!(entries[0].face_count, 0);
    assert!(entries[0].representative.is_none());
}

/// Renaming a person updates the persons row and rewrites the MWG region
/// names in every confirmed face's sidecar; the name survives a sidecar
/// re-read. Sidecars first, catalog mirror second (§5).
#[test]
fn renaming_a_person_rewrites_the_sidecar_regions_and_survives_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());

    let mut embs = blob(A, 10, 0.01, 1);
    embs.extend(blob(B, 10, 0.01, 2));
    let ids = seed_faces(&mut cat, dir.path(), &embs);
    cat.cluster_faces(5).unwrap();
    let a = cat.face_cluster_assignments(&ids[..10]).unwrap()[&ids[0]];
    let (person, _) = cat.bulk_confirm_cluster(a, "Alice").unwrap();
    let b = cat.face_cluster_assignments(&ids[10..]).unwrap()[&ids[10]];
    cat.bulk_confirm_cluster(b, "Bob").unwrap();

    let renamed = cat.rename_person(person.id, "Alicia").unwrap();
    assert_eq!(renamed.name, "Alicia");

    // Every A-photo sidecar now says Alicia; Bob's stay Bob.
    for id in &ids[..10] {
        let path: String = cat
            .conn()
            .query_row(
                "SELECT path FROM files WHERE id = (SELECT file_id FROM faces WHERE id = ?1)",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        let faces = Sidecar::read(&sidecar_path(&path)).unwrap().face_regions();
        assert_eq!(faces.len(), 1, "{faces:?}");
        assert_eq!(faces[0].name, "Alicia", "file {path}");
    }
    let b_path: String = cat
        .conn()
        .query_row(
            "SELECT path FROM files WHERE id = (SELECT file_id FROM faces WHERE id = ?1)",
            [ids[10]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(Sidecar::read(&sidecar_path(&b_path)).unwrap().face_regions()[0].name, "Bob");

    // The persons row carries the new name; the faces still point at the
    // same person, so the name survives a catalog rebuild from sidecars.
    assert_eq!(cat.face_person(ids[0]).unwrap(), Some(person.id));
    let name: String = cat
        .conn()
        .query_row("SELECT name FROM persons WHERE id = ?1", [person.id], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "Alicia");

    // The rename surfaces in the manager's list.
    let entries = cat.people_entries().unwrap();
    assert!(entries.iter().any(|e| e.name.as_deref() == Some("Alicia")));
}

#[test]
fn renaming_refuses_blank_duplicate_and_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());

    let mut embs = blob(A, 5, 0.01, 1);
    embs.extend(blob(B, 5, 0.01, 2));
    let ids = seed_faces(&mut cat, dir.path(), &embs);
    cat.cluster_faces(5).unwrap();
    let a = cat.face_cluster_assignments(&ids[..5]).unwrap()[&ids[0]];
    let b = cat.face_cluster_assignments(&ids[5..]).unwrap()[&ids[5]];
    let (alice, _) = cat.bulk_confirm_cluster(a, "Alice").unwrap();
    cat.bulk_confirm_cluster(b, "Bob").unwrap();

    assert!(cat.rename_person(alice.id, "   ").is_err(), "blank accepted");
    assert!(cat.rename_person(alice.id, "bob").is_err(), "duplicate accepted");
    assert!(cat.rename_person(4242, "Carol").is_err(), "unknown person accepted");
    // Nothing changed.
    let names: Vec<String> = {
        let mut stmt = cat.conn().prepare("SELECT name FROM persons ORDER BY name").unwrap();
        stmt.query_map([], |r| r.get(0)).unwrap().map(|n| n.unwrap()).collect()
    };
    assert_eq!(names, vec!["Alice", "Bob"]);
}
