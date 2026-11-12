//! TASK-07-001: CLIP photo embeddings in the catalog (PLAN §5 derived data,
//! §6 P3 Smart search). Embeddings live in the sqlite-vec `photo_embeddings`
//! table keyed by file id, the `files.embed_scanned` marker drives the
//! incremental pass, and nothing here ever touches a sidecar.

use fotobase_catalog::Catalog;

fn fixture(n: usize) -> (tempfile::TempDir, Catalog, Vec<i64>) {
    let lib = tempfile::tempdir().unwrap();
    let dir = lib.path().join("photos");
    std::fs::create_dir_all(&dir).unwrap();
    let db = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(db.path().join("catalog.db")).unwrap();
    cat.seed_folder(dir.to_str().unwrap()).unwrap();
    let folder_id: i64 =
        cat.conn().query_row("SELECT id FROM folders LIMIT 1", [], |r| r.get(0)).unwrap();
    for i in 0..n {
        cat.conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, media_type) \
                 VALUES (?1, ?2, ?3, 16, 0.0, 'image')",
                rusqlite::params![
                    folder_id,
                    dir.join(format!("p{i}.jpg")).to_str().unwrap(),
                    format!("p{i}.jpg")
                ],
            )
            .unwrap();
    }
    let ids: Vec<i64> = cat
        .conn()
        .prepare("SELECT id FROM files ORDER BY path")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .flatten()
        .collect();
    (lib, cat, ids)
}

fn unit_vector(seed: f32) -> Vec<f32> {
    let mut v: Vec<f32> = (0..fotobase_catalog::embeddings::CLIP_EMBEDDING_DIM)
        .map(|i| (i as f32 + seed).sin())
        .collect();
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    for x in &mut v {
        *x /= norm;
    }
    v
}

#[test]
fn embed_markers_start_pending_and_store_round_trips() {
    let (_lib, mut cat, ids) = fixture(2);
    let pending: usize = cat
        .conn()
        .query_row("SELECT COUNT(*) FROM files WHERE embed_scanned = 0", [], |r| r.get(0))
        .unwrap();
    assert_eq!(pending, 2, "every photo starts pending");

    cat.store_photo_embedding(ids[0], &unit_vector(0.0)).unwrap();
    cat.mark_embed_scanned(ids[0], true).unwrap();

    let stored = cat.photo_embedding(ids[0]).unwrap().expect("stored");
    let want = unit_vector(0.0);
    assert_eq!(stored.len(), want.len());
    for (a, b) in stored.iter().zip(&want) {
        assert!((a - b).abs() < 1e-5, "vector mismatch {a} vs {b}");
    }
    assert!(cat.photo_embedding(ids[1]).unwrap().is_none(), "not yet stored");
}

#[test]
fn wrong_dimension_embedding_is_refused() {
    let (_lib, mut cat, ids) = fixture(1);
    let err = cat
        .store_photo_embedding(ids[0], &vec![0.0; 128])
        .expect_err("128-d face embedding must not land in the CLIP table");
    assert!(err.to_string().contains("512-dimensional"), "unexpected: {err}");
    assert!(cat.photo_embedding(ids[0]).unwrap().is_none());
}

#[test]
fn embed_candidates_returns_only_pending_images() {
    let (_lib, mut cat, ids) = fixture(3);
    cat.mark_embed_scanned(ids[1], true).unwrap();
    let candidates = cat.embed_candidates().unwrap();
    assert_eq!(candidates.len(), 2, "only the pending photos");
    let got: Vec<i64> = candidates.iter().map(|(id, _)| *id).collect();
    assert_eq!(got, vec![ids[0], ids[2]], "oldest first, done row skipped");
}

#[test]
fn failed_marker_parks_the_row_and_reset_retries_it() {
    let (_lib, mut cat, ids) = fixture(1);
    cat.mark_embed_scanned(ids[0], false).unwrap();
    assert!(cat.embed_candidates().unwrap().is_empty(), "failed parks the row");
    cat.reset_embed_scan(ids[0]).unwrap();
    assert_eq!(cat.embed_candidates().unwrap().len(), 1, "reset retries");
}

#[test]
fn deleting_the_photo_drops_its_embedding() {
    // Derived data with no sidecar footprint: the vec0 row must not
    // outlive its file row (no orphaned vectors, no sidecar writes).
    let (_lib, mut cat, ids) = fixture(1);
    cat.store_photo_embedding(ids[0], &unit_vector(0.0)).unwrap();
    cat.remove_file(ids[0]).unwrap();
    assert!(cat.photo_embedding(ids[0]).unwrap().is_none(), "no orphan");
}
