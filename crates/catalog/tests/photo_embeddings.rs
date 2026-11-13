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
    let pending: i64 = cat
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

// TASK-07-002: the ANN query path — vec0 KNN over the stored photo
// embeddings, ranked by ascending cosine distance.

#[test]
fn knn_query_returns_rows_ordered_by_ascending_distance() {
    let (_lib, mut cat, ids) = fixture(3);
    // ids[0] closest to the query, ids[2] farthest (unit_vector is
    // deterministic per seed).
    cat.store_photo_embedding(ids[0], &unit_vector(0.0)).unwrap();
    cat.store_photo_embedding(ids[1], &unit_vector(2.0)).unwrap();
    cat.store_photo_embedding(ids[2], &unit_vector(4.0)).unwrap();

    let query = unit_vector(0.3);
    let hits = cat.search_photo_embeddings(&query, 3).unwrap();
    assert_eq!(hits.len(), 3, "all stored rows are candidates");
    let got: Vec<i64> = hits.iter().map(|(id, _)| *id).collect();
    assert_eq!(got, vec![ids[0], ids[1], ids[2]], "ascending distance");
    for (a, b) in hits.iter().zip(hits.iter().skip(1)) {
        assert!(a.1 <= b.1, "distances must be non-decreasing: {hits:?}");
    }
}

#[test]
fn knn_limit_k_returns_the_k_closest() {
    let (_lib, mut cat, ids) = fixture(4);
    // Explicit geometry: the query is e0; ids[0] is nearly e0, the rest
    // point the opposite way.
    let mut near = vec![0f32; fotobase_catalog::embeddings::CLIP_EMBEDDING_DIM];
    near[0] = 1.0;
    let mut far = vec![0f32; fotobase_catalog::embeddings::CLIP_EMBEDDING_DIM];
    far[1] = 1.0;
    cat.store_photo_embedding(ids[0], &near).unwrap();
    for id in &ids[1..] {
        cat.store_photo_embedding(*id, &far).unwrap();
    }
    let hits = cat.search_photo_embeddings(&near, 2).unwrap();
    assert_eq!(hits.len(), 2, "k limits the result");
    assert_eq!(hits[0].0, ids[0], "the exact match is first");
    assert!(hits[0].1 < hits[1].1, "second hit is a far vector");
}

#[test]
fn query_dimension_must_match_the_table() {
    let (_lib, cat, _ids) = fixture(1);
    let err = cat
        .search_photo_embeddings(&vec![0.0; 128], 5)
        .expect_err("a 128-d query cannot match a 512-d vec0 table");
    assert!(err.to_string().contains("512"), "unexpected: {err}");
}

/// The ranking integration test from the TDD criteria, run against
/// synthetic vectors: the "dog on the beach" photo has the query vector
/// closest to the text embedding, so it must rank first despite a
/// distractor set. The real text encoder is exercised by the
/// perf-acceptance harness (the model is an opt-in download, never in CI).
#[test]
fn text_query_ranks_the_matching_photo_first_across_distractors() {
    let (_lib, mut cat, ids) = fixture(6);
    // ids[0]: "dog on a beach" — closest to the text query.
    // ids[1..6]: distractors (interiors, food, cars, …).
    cat.store_photo_embedding(ids[0], &unit_vector(0.0)).unwrap();
    for (i, id) in ids.iter().enumerate().skip(1) {
        cat.store_photo_embedding(*id, &unit_vector(10.0 + i as f32)).unwrap();
    }
    let dog_query = unit_vector(0.2);
    let hits = cat.search_photo_embeddings(&dog_query, 3).unwrap();
    assert_eq!(hits[0].0, ids[0], "the matching photo ranks first");
    assert!(hits[0].1 < hits[1].1, "matching photo strictly closer than the best distractor");
}

/// The 50k-fixture perf criterion (PLAN §9, < ~500 ms). `#[ignore]`d —
/// 100 MB of vectors is too heavy for the CI suite; run with
/// `cargo test -p fotobase-catalog --test photo_embeddings -- --ignored`.
#[test]
#[ignore]
fn ann_query_on_50k_vectors_under_500ms() {
    let (_lib, mut cat, ids) = fixture(50_000);
    let mut v = unit_vector(0.0);
    for id in &ids {
        // Small deterministic perturbation per row.
        for x in &mut v {
            *x = (*x + 0.1).tanh();
        }
        cat.store_photo_embedding(*id, &v).unwrap();
    }
    let query = unit_vector(1.0);
    let start = std::time::Instant::now();
    let hits = cat.search_photo_embeddings(&query, 20).unwrap();
    let elapsed = start.elapsed();
    eprintln!("50k ANN query: {elapsed:?} (budget 500 ms)");
    assert_eq!(hits.len(), 20);
    assert!(elapsed.as_millis() < 500, "50k ANN query took {elapsed:?}, budget 500 ms");
}

/// TASK-07-002: a full catalog rebuild re-derives embeddings — the vec0
/// table is cleared and every photo returns to the pending marker so the
/// next pass recomputes it (PLAN §9, §3 rule 2).
#[test]
fn full_rebuild_resets_the_embedding_derived_data() {
    let (_lib, mut cat, ids) = fixture(2);
    cat.store_photo_embedding(ids[0], &unit_vector(0.0)).unwrap();
    cat.mark_embed_scanned(ids[0], true).unwrap();
    cat.mark_embed_scanned(ids[1], false).unwrap();

    let stored: i64 =
        cat.conn().query_row("SELECT COUNT(*) FROM photo_embeddings", [], |r| r.get(0)).unwrap();
    assert_eq!(stored, 1);

    cat.reset_all_photo_embeddings().unwrap();

    let stored: i64 =
        cat.conn().query_row("SELECT COUNT(*) FROM photo_embeddings", [], |r| r.get(0)).unwrap();
    assert_eq!(stored, 0, "vec0 table cleared");
    assert_eq!(cat.embed_candidates().unwrap().len(), 2, "all photos pending again");
}

#[test]
fn full_rebuild_from_sidecars_resets_embeddings() {
    let (_lib, mut cat, ids) = fixture(2);
    cat.store_photo_embedding(ids[0], &unit_vector(0.0)).unwrap();
    cat.mark_embed_scanned(ids[0], true).unwrap();

    cat.rebuild_from_sidecars(true).unwrap();

    let stored: i64 =
        cat.conn().query_row("SELECT COUNT(*) FROM photo_embeddings", [], |r| r.get(0)).unwrap();
    assert_eq!(stored, 0, "full rebuild cleared the vec0 table");
    assert_eq!(cat.embed_candidates().unwrap().len(), 2, "all pending again");

    // Incremental rebuild leaves the derived data alone.
    cat.store_photo_embedding(ids[0], &unit_vector(0.0)).unwrap();
    cat.mark_embed_scanned(ids[0], true).unwrap();
    cat.rebuild_from_sidecars(false).unwrap();
    assert!(cat.photo_embedding(ids[0]).unwrap().is_some(), "partial rebuild keeps embeddings");
}
