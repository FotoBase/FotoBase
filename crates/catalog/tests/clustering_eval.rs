//! TASK-05-012: clustering precision/recall evaluation on a labeled face
//! set (PLAN §9 AI row, §6 P3 People acceptance). The labeled set is
//! generated deterministically (seeded xorshift — no external rng, same
//! criterion as `face_clusters.rs`): every embedding carries a known
//! identity, plus deliberate noise outliers that belong to no identity.
//!
//! The harness clusters through the production path
//! (`Catalog::cluster_faces`, HDBSCAN) and scores the partition with
//! pairwise precision/recall: two faces are a true pair iff they share an
//! identity; a predicted pair is two faces the clustering put in the same
//! cluster. Noise faces predict no pair. Numbers are compared against the
//! recorded baselines in `fixtures/clustering-baseline.json` — a drop
//! below a recorded floor fails the test (regression flagged), and the
//! measured numbers are printed as JSON so CI reports them (§9 "reported
//! in CI with recorded numbers").

use fotobase_catalog::{Catalog, FaceRecord};

const IDENTITIES: usize = 10;
const FACES_PER_IDENTITY: usize = 10;
const NOISE_FACES: usize = 8;
/// HDBSCAN minimum cluster size — above the per-identity count is
/// impossible, well below it so every identity forms its own cluster.
const MIN_CLUSTER_SIZE: usize = 5;

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

fn face() -> FaceRecord {
    FaceRecord {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 120.0,
        landmarks: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
        confidence: 0.9,
    }
}

/// xorshift64* PRNG, seeded — the labeled set is identical on every run
/// and every platform (determinism criterion, §9).
struct Rng(u64);

impl Rng {
    fn next_unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as f32 / u64::MAX as f32 - 0.5
    }
}

/// The labeled fixture: `(identity, embedding)` pairs. Identity `None` is
/// a noise face. Embeddings vary only in the first 8 components (the same
/// trick `face_clusters.rs` uses: full-dimension uniform noise concentrates
/// pairwise distances, while real unit-norm embeddings live on a
/// low-dimensional manifold).
fn labeled_set() -> Vec<(usize, [f32; 128])> {
    let mut rng = Rng(0x0005_012C_AFEF_00D1);
    let mut out = Vec::new();
    // Identity centroids: unit-ish vectors far apart (each on its own
    // axis block), so blobs never touch.
    for id in 0..IDENTITIES {
        let mut center = [0.5f32; 128];
        center[id * 8] = 1.5;
        for _ in 0..FACES_PER_IDENTITY {
            let mut v = center;
            for c in v.iter_mut().take(8) {
                *c += rng.next_unit() * 0.02;
            }
            out.push((id, v));
        }
    }
    // Noise faces: scattered far from every centroid. Each noise face is
    // its own identity — a spurious cluster over them is a real precision
    // loss, not a scoring artifact.
    for k in 0..NOISE_FACES {
        let mut v = [0.0f32; 128];
        for c in v.iter_mut().take(8) {
            *c = rng.next_unit() * 4.0;
        }
        out.push((IDENTITIES + k, v));
    }
    out
}

/// Pairwise precision/recall of a clustering against ground-truth
/// identities. Returns (precision, recall, true_pairs, predicted_pairs).
fn pairwise_scores(labels: &[(usize, Option<i64>)]) -> (f64, f64, u64, u64) {
    let mut tp = 0u64;
    let mut predicted = 0u64;
    let mut actual = 0u64;
    for i in 0..labels.len() {
        for j in i + 1..labels.len() {
            let same_truth = labels[i].0 == labels[j].0;
            let same_pred = labels[i].1.is_some() && labels[i].1 == labels[j].1;
            if same_truth {
                actual += 1;
            }
            if same_pred {
                predicted += 1;
            }
            if same_truth && same_pred {
                tp += 1;
            }
        }
    }
    let precision = tp as f64 / predicted.max(1) as f64;
    let recall = tp as f64 / actual.max(1) as f64;
    (precision, recall, actual, predicted)
}

/// The recorded baseline: measured numbers plus regression floors.
#[derive(serde::Deserialize)]
struct Baseline {
    precision: f64,
    recall: f64,
    precision_floor: f64,
    recall_floor: f64,
}

fn baseline() -> Baseline {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/clustering-baseline.json");
    serde_json::from_slice(&std::fs::read(&path).expect("baseline fixture checked in")).unwrap()
}

#[test]
fn labeled_set_scores_against_recorded_baselines() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let set = labeled_set();
    let offset =
        { cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get::<_, i64>(0)).unwrap() };
    for (i, (_, v)) in set.iter().enumerate() {
        let file_id = add_photo(&mut cat, dir.path(), &format!("p{}.jpg", offset + i as i64));
        cat.record_face(file_id, &face(), Some(v)).unwrap();
    }

    cat.cluster_faces(MIN_CLUSTER_SIZE).unwrap();

    // Ground truth + predicted cluster per face, in the deterministic
    // face-id order the harness created them in.
    let rows: Vec<(usize, Option<i64>)> = cat
        .conn()
        .prepare("SELECT f.id, f.cluster FROM faces f ORDER BY f.id")
        .unwrap()
        .query_map([], |r| {
            let id: i64 = r.get(0)?;
            let cluster: Option<i64> = r.get(1)?;
            Ok((id, cluster))
        })
        .unwrap()
        .flatten()
        .skip(offset as usize)
        .zip(set.iter())
        .map(|((_, cluster), (truth, _))| (*truth, cluster))
        .collect();
    assert_eq!(rows.len(), set.len());

    let (precision, recall, actual, predicted) = pairwise_scores(&rows);
    let base = baseline();
    println!(
        "clustering eval: precision={precision:.4} recall={recall:.4} \
         true_pairs={actual} predicted_pairs={predicted} (baseline {}/{})",
        base.precision, base.recall
    );
    assert!(
        precision >= base.precision_floor,
        "clustering precision {precision:.4} below recorded floor {} — regression",
        base.precision_floor
    );
    assert!(
        recall >= base.recall_floor,
        "clustering recall {recall:.4} below recorded floor {} — regression",
        base.recall_floor
    );
}
