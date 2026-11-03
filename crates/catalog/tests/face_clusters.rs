//! TASK-05-003: HDBSCAN face clustering (PLAN §6 P3 People, §5 derived-data
//! domain, §4 Clustering). Clusters are derived data — recomputable from
//! embeddings, never written to sidecars (PLAN §3 rule 2). The `hdbscan`
//! crate (MIT/Apache-2.0, pure Rust) does the clustering; persistence and
//! incremental assignment live in the catalog.

use fotobase_catalog::{Catalog, FaceRecord};

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
        width: 100.0,
        height: 120.0,
        landmarks: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
        confidence: 0.9,
    }
}

/// Two tight gaussian blobs far apart — the canonical "two people" test set.
/// Seeded so every run builds the same embeddings (determinism criterion).
fn blob(center: [f32; 8], n: usize, spread: f32, seed: u64) -> Vec<[f32; 8]> {
    // xorshift64* — no external rng dependency, fully deterministic.
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

/// Seed `cat` with faces carrying the given embeddings. Returns the face ids.
fn seed_faces(cat: &mut Catalog, dir: &std::path::Path, embeddings: &[[f32; 8]]) -> Vec<i64> {
    // Unique names across calls: each seeding batch starts after the
    // photos already in the catalog.
    let offset =
        { cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get::<_, i64>(0)).unwrap() };
    embeddings
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let file_id = add_photo(cat, dir, &format!("p{}.jpg", offset + i as i64));
            cat.record_face(file_id, &face(0.0, 0.0), Some(v)).unwrap()
        })
        .collect()
}

/// A third well-separated blob, used to grow the library incrementally.
fn more_embeddings(n: usize, center: [f32; 8], seed: u64) -> Vec<[f32; 8]> {
    blob(center, n, 0.01, seed)
}

const A: [f32; 8] = [-1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0];
const B: [f32; 8] = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
const C: [f32; 8] = [4.0, -4.0, 4.0, -4.0, 4.0, -4.0, 4.0, -4.0];

#[test]
fn faces_in_same_blob_cluster_together() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());

    let mut embeddings = blob(A, 25, 0.01, 1);
    embeddings.extend(blob(B, 25, 0.01, 2));
    let ids = seed_faces(&mut cat, dir.path(), &embeddings);

    cat.cluster_faces(5).unwrap();
    let assignments = cat.face_cluster_assignments(&ids).unwrap();

    // Every face lands in some cluster: no noise when the data is clean.
    for id in &ids {
        assert!(assignments.contains_key(id), "face {id} unassigned");
    }
    // The two blobs must not mix: first half and second half disjoint.
    let first: std::collections::HashSet<_> = ids[..25].iter().map(|i| assignments[i]).collect();
    let second: std::collections::HashSet<_> = ids[25..].iter().map(|i| assignments[i]).collect();
    assert!(first.is_disjoint(&second), "blobs mixed: {first:?} vs {second:?}");
    // Within a blob, one cluster (allowing a stray outlier cluster is not
    // tolerated here — spread 0.01 around unit-scale centers is clean data).
    assert_eq!(first.len(), 1, "blob A fragmented: {first:?}");
    assert_eq!(second.len(), 1, "blob B fragmented: {second:?}");
}

#[test]
fn clustering_is_deterministic_across_runs() {
    let embeddings: Vec<[f32; 8]> = blob(A, 25, 0.01, 1)
        .into_iter()
        .chain(blob(B, 25, 0.01, 2))
        .chain(blob(C, 10, 0.01, 3))
        .collect();

    let labels: Vec<Vec<Option<i64>>> = (0..3)
        .map(|_| {
            let dir = tempfile::tempdir().unwrap();
            let mut cat = open(dir.path());
            let ids = seed_faces(&mut cat, dir.path(), &embeddings);
            cat.cluster_faces(5).unwrap();
            let a = cat.face_cluster_assignments(&ids).unwrap();
            ids.iter().map(|i| a.get(i).copied()).collect()
        })
        .collect();

    // Same input → same partition (cluster ids are rowids, so compare the
    // partition structure: pairwise same/different across all three runs).
    let partition = |labels: &Vec<Option<i64>>| -> Vec<Vec<usize>> {
        let mut groups: std::collections::BTreeMap<Option<i64>, Vec<usize>> = Default::default();
        for (i, l) in labels.iter().enumerate() {
            groups.entry(*l).or_default().push(i);
        }
        groups.into_values().collect()
    };
    let p0 = partition(&labels[0]);
    for p in &labels[1..] {
        assert_eq!(p0, partition(p), "clustering not deterministic");
    }
}

#[test]
fn clusters_persist_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let embeddings = blob(A, 20, 0.01, 1);
    let ids;
    {
        let mut cat = open(dir.path());
        ids = seed_faces(&mut cat, dir.path(), &embeddings);
        cat.cluster_faces(5).unwrap();
    }
    let cat = open(dir.path());
    let assignments = cat.face_cluster_assignments(&ids).unwrap();
    // HDBSCAN may mark a few borderline points noise; the partition must
    // survive the reopen, not every single point.
    assert!(
        assignments.len() >= ids.len() - 5,
        "clusters lost on reopen: got {} of {}",
        assignments.len(),
        ids.len()
    );
    let all: std::collections::HashSet<_> = ids.iter().filter_map(|i| assignments.get(i)).collect();
    assert_eq!(all.len(), 1, "one blob = one cluster after reopen");
}

#[test]
fn incremental_update_touches_only_new_faces() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());

    let base: Vec<[f32; 8]> =
        blob(A, 50, 0.01, 1).into_iter().chain(blob(B, 50, 0.01, 2)).collect();
    let base_ids = seed_faces(&mut cat, dir.path(), &base);
    cat.cluster_faces(5).unwrap();
    let before = cat.face_cluster_assignments(&base_ids).unwrap();

    // 10 new faces: 6 join blob A, 4 form a brand-new blob C.
    let new: Vec<[f32; 8]> =
        more_embeddings(6, A, 11).into_iter().chain(more_embeddings(4, C, 12)).collect();
    let new_ids = seed_faces(&mut cat, dir.path(), &new);

    cat.cluster_faces_incremental(1.0).unwrap();

    // Existing assignments unchanged — no full re-cluster.
    let after = cat.face_cluster_assignments(&base_ids).unwrap();
    assert_eq!(before, after, "incremental pass rewrote existing clusters");

    let assignments = cat.face_cluster_assignments(&new_ids).unwrap();
    for id in &new_ids {
        assert!(assignments.contains_key(id), "new face {id} unassigned");
    }
    // The 6 A-like faces joined the existing A cluster; the 4 C faces
    // spawned a cluster disjoint from A and B.
    let a_cluster = before[&base_ids[0]];
    let joined = assignments[&new_ids[0]];
    assert_eq!(joined, a_cluster, "near-duplicate of A did not join A");
    let c_cluster = assignments[&new_ids[6]];
    assert_ne!(c_cluster, a_cluster, "new blob merged into A");
    let b_cluster = before[&base_ids[50]];
    assert_ne!(c_cluster, b_cluster, "new blob merged into B");
}

/// Outliers (noise far from any blob) stay unassigned — HDBSCAN's core
/// benefit over k-means: no forced cluster membership.
#[test]
fn outliers_are_not_forced_into_clusters() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());

    let mut embeddings = blob(A, 20, 0.01, 1);
    // Far-away singletons.
    embeddings.push([-50.0, 50.0, -50.0, 50.0, -50.0, 50.0, -50.0, 50.0]);
    embeddings.push([50.0, -50.0, 50.0, -50.0, 50.0, -50.0, 50.0, -50.0]);
    let ids = seed_faces(&mut cat, dir.path(), &embeddings);

    cat.cluster_faces(5).unwrap();
    let assignments = cat.face_cluster_assignments(&ids).unwrap();

    let blob_ids: std::collections::HashSet<_> =
        ids[..20].iter().filter_map(|i| assignments.get(i)).collect();
    assert_eq!(blob_ids.len(), 1, "blob fragmented: {blob_ids:?}");
    for id in &ids[20..] {
        assert_ne!(
            assignments.get(id).copied(),
            blob_ids.iter().next().copied().copied(),
            "outlier {id} absorbed into the blob"
        );
    }
}

/// Labeled evaluation: faces we know belong to person A vs person B must
/// separate — precision/recall proxy for the §9 AI check on a labeled set.
#[test]
fn labeled_set_separates_known_persons() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());

    // 30 A-faces, 30 B-faces, plus 20 A-faces that arrive by import later.
    let labeled: Vec<[f32; 8]> =
        blob(A, 30, 0.01, 5).into_iter().chain(blob(B, 30, 0.01, 6)).collect();
    let labeled_ids = seed_faces(&mut cat, dir.path(), &labeled);
    cat.cluster_faces(5).unwrap();

    let unlabeled = more_embeddings(20, A, 7);
    let unlabeled_ids = seed_faces(&mut cat, dir.path(), &unlabeled);
    cat.cluster_faces_incremental(1.0).unwrap();

    let a_labels = &labeled_ids[..30];
    let b_labels = &labeled_ids[30..];
    let a_assign = cat.face_cluster_assignments(a_labels).unwrap();
    let b_assign = cat.face_cluster_assignments(b_labels).unwrap();
    let new_assign = cat.face_cluster_assignments(&unlabeled_ids).unwrap();

    let a_cluster = a_assign[&a_labels[0]];
    let b_cluster = b_assign[&b_labels[0]];
    assert_ne!(a_cluster, b_cluster);
    // Precision / recall on the unlabeled A-faces: all 20 must land in the
    // A cluster (tight synthetic data; §9's real labeled set exercises this
    // through the perf harness, this is the CI-scale proxy).
    let hits = unlabeled_ids.iter().filter(|i| new_assign[*i] == a_cluster).count();
    assert_eq!(hits, unlabeled_ids.len(), "only {hits}/20 A-faces recalled");
}

#[test]
fn clustering_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let embeddings: Vec<[f32; 8]> =
        blob(A, 20, 0.01, 1).into_iter().chain(blob(B, 20, 0.01, 2)).collect();
    let ids = seed_faces(&mut cat, dir.path(), &embeddings);

    cat.cluster_faces(5).unwrap();
    let first = cat.face_cluster_assignments(&ids).unwrap();
    cat.cluster_faces(5).unwrap();
    let second = cat.face_cluster_assignments(&ids).unwrap();
    assert_eq!(first, second, "re-clustering changed the partition");
}
