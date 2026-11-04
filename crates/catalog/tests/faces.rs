//! TASK-05-002 / TASK-05-010: the face-detection catalog side (PLAN §6 P3
//! People, §5 derived-data domain). The per-folder toggle lives in
//! `folders.face_detect`; faces and embeddings are rebuildable cache rows —
//! never written to sidecars (PLAN §3 rule 2).

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

const FACE: FaceRecord = FaceRecord {
    x: 10.0,
    y: 20.0,
    width: 100.0,
    height: 120.0,
    landmarks: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
    confidence: 0.9,
};

#[test]
fn face_detection_defaults_to_off_per_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    cat.seed_folder("/photos").unwrap();
    assert_eq!(cat.folder_face_detection("/photos").unwrap(), Some(false));
}

#[test]
fn toggle_is_persisted_per_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    cat.seed_folder("/photos").unwrap();
    cat.seed_folder("/archive").unwrap();

    assert!(cat.set_folder_face_detection("/photos", true).unwrap());
    assert_eq!(cat.folder_face_detection("/photos").unwrap(), Some(true));
    assert_eq!(cat.folder_face_detection("/archive").unwrap(), Some(false));

    // Back off again: the toggle is a real switch, not one-way.
    assert!(cat.set_folder_face_detection("/photos", false).unwrap());
    assert_eq!(cat.folder_face_detection("/photos").unwrap(), Some(false));
}

#[test]
fn unknown_folder_toggles_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let cat = open(dir.path());
    assert!(!cat.set_folder_face_detection("/nope", true).unwrap());
    assert_eq!(cat.folder_face_detection("/nope").unwrap(), None);
}

#[test]
fn list_folders_carries_the_toggle() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    cat.seed_folder("/photos").unwrap();
    cat.set_folder_face_detection("/photos", true).unwrap();
    let folders = cat.list_folders().unwrap();
    assert_eq!(folders.len(), 1);
    assert!(folders[0].face_detect);
}

#[test]
fn candidates_come_only_from_enabled_folders() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let on_dir = dir.path().join("on");
    let off_dir = dir.path().join("off");
    std::fs::create_dir_all(&on_dir).unwrap();
    std::fs::create_dir_all(&off_dir).unwrap();
    let on = cat.seed_folder(on_dir.to_str().unwrap()).unwrap();
    let off = cat.seed_folder(off_dir.to_str().unwrap()).unwrap();
    let _ = off;
    let a = add_photo(&mut cat, &on_dir, "a.jpg");
    add_photo(&mut cat, &off_dir, "b.jpg");

    // Nothing enabled yet: no candidates at all.
    assert!(cat.face_candidates(&cat.face_enabled_folder_ids().unwrap()).unwrap().is_empty());

    cat.set_folder_face_detection(on_dir.to_str().unwrap(), true).unwrap();
    let ids = cat.face_enabled_folder_ids().unwrap();
    assert_eq!(ids, vec![on]);
    let candidates = cat.face_candidates(&ids).unwrap();
    assert_eq!(candidates, vec![(a, on_dir.join("a.jpg").to_str().unwrap().to_string())]);

    // `off` is not in the enabled set, so its photo is never scheduled.
    assert!(!ids.contains(&off));
}

#[test]
fn processed_files_leave_the_candidate_set() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    cat.seed_folder("/photos").unwrap();
    cat.set_folder_face_detection("/photos", true).unwrap();
    let a = add_photo(&mut cat, dir.path(), "a.jpg");

    // A pass with zero faces still marks the file done — a photo with no
    // faces must not be re-detected on every pass.
    cat.mark_face_scanned(a, true).unwrap();
    assert!(cat.face_candidates(&[1]).unwrap().is_empty());

    // A failed decode is parked too (sentinel), so one corrupt file never
    // blocks the pass forever — but a later explicit retry can reset it.
    let b = add_photo(&mut cat, dir.path(), "b.jpg");
    cat.mark_face_scanned(b, false).unwrap();
    assert!(cat.face_candidates(&[1]).unwrap().is_empty());
}

/// A deterministic 128-dimensional embedding (TASK-05-008: the sqlite-vec
/// table is fixed-width, the SFace dimension).
fn e128(seed: u64) -> Vec<f32> {
    (0..128).map(|i| ((i as u64 ^ seed) % 7) as f32 / 7.0 - 0.5).collect()
}

#[test]
fn faces_and_embeddings_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    cat.seed_folder("/photos").unwrap();
    let a = add_photo(&mut cat, dir.path(), "a.jpg");

    let vector = e128(3);
    let id = cat.record_face(a, &FACE, Some(&vector)).unwrap();
    assert!(id > 0);

    let rows = cat.faces_for_file(a).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].x, FACE.x);
    assert_eq!(rows[0].confidence, FACE.confidence);
    assert_eq!(rows[0].landmarks, Some(FACE.landmarks.to_vec()));

    let (dim, got) = cat.face_embedding(id).unwrap().unwrap();
    assert_eq!(dim, 128);
    assert_eq!(got, vector);

    // A face without an embedding is legal (detect succeeded, embed pending).
    let b = add_photo(&mut cat, dir.path(), "b.jpg");
    let id2 = cat.record_face(b, &FACE, None).unwrap();
    assert_eq!(cat.face_embedding(id2).unwrap(), None);
    assert!(cat.faces_for_file(a).unwrap()[0].id != cat.faces_for_file(b).unwrap()[0].id);
}

/// TASK-05-008: the embeddings table is a sqlite-vec virtual table keyed to
/// `faces.embedding_id` (PLAN §5), and a stored vector reads back unchanged
/// (store → query → same vector).
#[test]
fn embeddings_live_in_the_sqlite_vec_virtual_table() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    cat.seed_folder("/photos").unwrap();
    let a = add_photo(&mut cat, dir.path(), "a.jpg");

    let vector = e128(9);
    let id = cat.record_face(a, &FACE, Some(&vector)).unwrap();

    let sql: String = cat
        .conn()
        .query_row("SELECT sql FROM sqlite_master WHERE name = 'embeddings'", [], |r| r.get(0))
        .unwrap();
    assert!(sql.contains("vec0"), "virtual table, not a plain BLOB table: {sql}");

    let (embedding_id, stored): (i64, Vec<u8>) = cat
        .conn()
        .query_row(
            "SELECT embedding_id, embedding FROM embeddings WHERE embedding_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(embedding_id, id, "faces.embedding_id keys the vec0 row");
    let stored: Vec<f32> =
        stored.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    assert_eq!(stored, vector, "store → query → same vector");

    // Similarity search works over the virtual table (PLAN §5).
    let probe: Vec<u8> = vector.iter().flat_map(|f| f.to_le_bytes()).collect();
    let hits: Vec<(i64, f32)> = cat
        .conn()
        .prepare(
            "SELECT embedding_id, distance FROM embeddings \
                  WHERE embedding MATCH ?1 AND k = 5",
        )
        .unwrap()
        .query_map(rusqlite::params![probe], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].0, id);
    assert!(hits[0].1.abs() < 1e-5, "self-match distance ≈ 0: {hits:?}");
}

#[test]
fn embeddings_must_match_the_model_dimension() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    cat.seed_folder("/photos").unwrap();
    let a = add_photo(&mut cat, dir.path(), "a.jpg");

    assert!(cat.record_face(a, &FACE, Some(&[0.5, -0.5, 1.0])).is_err());
    assert!(cat.face_embedding(1).unwrap().is_none());
}

#[test]
fn deleting_a_file_drops_its_faces_and_embeddings() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    cat.seed_folder("/photos").unwrap();
    let a = add_photo(&mut cat, dir.path(), "a.jpg");
    let id = cat.record_face(a, &FACE, Some(&e128(5))).unwrap();
    assert!(cat.remove_file(a).unwrap());
    assert!(cat.faces_for_file(a).unwrap().is_empty());
    // The vec0 table has no foreign-key cascade: the embedding row goes too.
    assert!(cat.face_embedding(id).unwrap().is_none());
}
