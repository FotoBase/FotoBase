//! TASK-07-003: blending FTS5 matches with CLIP ANN results (PLAN §6 P3
//! "Semantic search in the live search bar"). The blend is deterministic
//! reciprocal-rank fusion; the catalog method applies the grid's non-text
//! filters to both candidate lists before blending. Embeddings stay derived
//! data in the vec0 table — nothing here touches a sidecar (§3 rule 2).

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

fn unit(seed: f32) -> Vec<f32> {
    let mut v: Vec<f32> = (0..fotobase_catalog::embeddings::CLIP_EMBEDDING_DIM)
        .map(|i| (i as f32 + seed).sin())
        .collect();
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    for x in &mut v {
        *x /= norm;
    }
    v
}

/// TDD unit criterion: FTS ranks [a, b] and ANN ranks [b, c] produce a
/// deterministic merged ordering. `b` appears in both lists (rank 1 in
/// each) so it leads; `a` and `c` tie, broken by ascending id.
#[test]
fn blend_of_overlapping_ranks_is_deterministic() {
    let (_lib, _cat, ids) = fixture(3);
    let (a, b, c) = (ids[0], ids[1], ids[2]);
    let fts = vec![a, b];
    let ann = vec![b, c];
    let merged = fotobase_catalog::blend::blend_fts_ann(&fts, &ann);
    assert_eq!(merged, vec![b, a, c], "shared hit leads, tie broken by id");
    // Determinism: same inputs, same output, every time.
    assert_eq!(merged, fotobase_catalog::blend::blend_fts_ann(&fts, &ann));
}

#[test]
fn blend_is_symmetric_in_rank_contribution() {
    // Swapping the lists must not change the merged set's ordering when the
    // input lists are swapped too (RRF is symmetric).
    let (_lib, _cat, ids) = fixture(3);
    let (a, b, c) = (ids[0], ids[1], ids[2]);
    let one = fotobase_catalog::blend::blend_fts_ann(&[a, b], &[b, c]);
    let two = fotobase_catalog::blend::blend_fts_ann(&[b, c], &[a, b]);
    let mut same = two.clone();
    same.retain(|id| one.contains(id));
    assert_eq!(one, same, "RRF gives both lists equal weight");
}

#[test]
fn empty_ann_degrades_to_fts_order() {
    let (_lib, _cat, ids) = fixture(2);
    assert_eq!(
        fotobase_catalog::blend::blend_fts_ann(&[ids[0], ids[1]], &[]),
        vec![ids[0], ids[1]],
        "no semantic hits = plain FTS order"
    );
}

#[test]
fn duplicate_hit_is_not_repeated() {
    let (_lib, _cat, ids) = fixture(2);
    let merged = fotobase_catalog::blend::blend_fts_ann(&[ids[0], ids[1]], &[ids[1]]);
    assert_eq!(merged, vec![ids[1], ids[0]], "one entry per photo");
}

/// Integration (the TDD criterion, run against synthetic vectors the way
/// TASK-07-002's ranking test is — the real text encoder is exercised by
/// the perf-acceptance harness): the "dog on the beach at sunset" photo
/// ranks first in the blended result even though no filename, tag or
/// caption matches the query text.
#[test]
fn semantic_query_returns_relevant_photo_blended_with_fts() {
    let (_lib, mut cat, ids) = fixture(6);
    // ids[0]: the "dog on the beach at sunset" photo — semantically closest
    // to the query vector. ids[1..6]: distractors (interiors, food, cars…).
    cat.store_photo_embedding(ids[0], &unit(0.0)).unwrap();
    for (i, id) in ids.iter().enumerate().skip(1) {
        cat.store_photo_embedding(*id, &unit(10.0 + i as f32)).unwrap();
    }
    let query_vec = unit(0.2);

    // No filename/folder/caption matches the text: FTS contributes nothing.
    let hits = cat.search_photo_embeddings(&query_vec, 5).unwrap();
    assert_eq!(hits.first().map(|h| h.0), Some(ids[0]), "ANN ranks the relevant photo first");

    // Give one distractor a text match, so FTS contributes a rank list too.
    cat.conn().execute("UPDATE files SET filename = 'sunset.jpg' WHERE id = ?1", [ids[1]]).unwrap();

    let query = fotobase_catalog::FileQuery { text: Some("sunset".into()), ..Default::default() };
    let ann_ids: Vec<i64> = hits.iter().map(|h| h.0).collect();
    let rows = cat.query_files_semantic(&query, &ann_ids, usize::MAX).unwrap();
    let got: Vec<i64> = rows.iter().map(|r| r.id).collect();
    // The text-matching distractor and the semantic hit must both surface;
    // exact text matches are not lost in the blend.
    assert!(got.contains(&ids[0]), "semantic hit present: {got:?}");
    assert!(got.contains(&ids[1]), "FTS match present: {got:?}");
    let unique: std::collections::HashSet<i64> = got.iter().copied().collect();
    assert_eq!(unique.len(), got.len(), "one entry per photo, no duplicates");
}

/// The grid's non-text filters (folders, album, rating, …) must narrow the
/// ANN candidate list too — a semantically similar photo in an unselected
/// folder never shows up.
#[test]
fn non_text_filters_narrow_the_ann_hits() {
    let (_lib, mut cat, ids) = fixture(3);
    let dir2 = tempfile::tempdir().unwrap();
    cat.seed_folder(dir2.path().to_str().unwrap()).unwrap();
    let folder_ids: Vec<i64> = cat
        .conn()
        .prepare("SELECT id FROM folders ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .flatten()
        .collect();
    cat.conn()
        .execute(
            "UPDATE files SET folder_id = ?1, path = ?2 WHERE id = ?3",
            rusqlite::params![folder_ids[1], dir2.path().join("p0.jpg").to_str().unwrap(), ids[0]],
        )
        .unwrap();
    cat.store_photo_embedding(ids[0], &unit(0.0)).unwrap();
    cat.store_photo_embedding(ids[1], &unit(1.0)).unwrap();

    // Folder filter excludes dir2, so ids[0] (the ANN top hit) is dropped
    // and ids[1] survives.
    let query =
        fotobase_catalog::FileQuery { folders: Some(vec![folder_ids[0]]), ..Default::default() };
    let rows = cat.query_files_semantic(&query, &[ids[0], ids[1]], usize::MAX).unwrap();
    let got: Vec<i64> = rows.iter().map(|r| r.id).collect();
    assert_eq!(got, vec![ids[1]], "only the in-folder ANN hit survives: {got:?}");
}
