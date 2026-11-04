//! TASK-05-009: "Reset faces" per photo (PLAN §2 Picture context menu, §6 P3
//! People). Resetting clears all detected/named face regions for one photo:
//! the sidecar's MWG face regions go (sidecar-first per
//! docs/sidecar-merge-on-write.md), then the `faces` rows and their
//! embeddings — and with them, via the trigger-maintained derivations, the
//! person-album membership, the FTS `people` column and the "has faces"
//! filter. Other photos and other sidecar content stay untouched.

use fotobase_catalog::{sidecar_path, Catalog, FaceRecord, FileQuery};
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
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type, width, height)
             VALUES (?1, ?2, ?3, 10, 0.0, 'image', 2000, 1000)",
            rusqlite::params![folder_id, original.to_str().unwrap(), name],
        )
        .unwrap();
    cat.conn().last_insert_rowid()
}

fn face() -> FaceRecord {
    FaceRecord {
        x: 0.1,
        y: 0.2,
        width: 0.2,
        height: 0.25,
        landmarks: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
        confidence: 0.9,
    }
}

fn blob(center: [f32; 128], n: usize, seed: u64) -> Vec<[f32; 128]> {
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
            for c in v.iter_mut().take(8) {
                *c += rnd() * 0.01;
            }
            v
        })
        .collect()
}

const A: [f32; 128] = [-1.0; 128];
const B: [f32; 128] = [1.0; 128];

/// Two clusters of five photos each; cluster A named "Anna".
fn seed() -> (tempfile::TempDir, Catalog, i64, i64, i64) {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let mut embs = blob(A, 5, 1);
    embs.extend(blob(B, 5, 2));
    let mut ids = Vec::new();
    for (i, v) in embs.iter().enumerate() {
        let file_id = add_photo(&mut cat, dir.path(), &format!("p{i}.jpg"));
        cat.record_face(file_id, &face(), Some(v)).unwrap();
        ids.push(file_id);
    }
    cat.cluster_faces(5).unwrap();
    let face_ids: Vec<i64> = cat
        .conn()
        .prepare("SELECT id FROM faces ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .flatten()
        .collect();
    let cluster_a = cat.face_cluster_assignments(&face_ids[..5]).unwrap()[&face_ids[0]];
    cat.name_cluster(cluster_a, "Anna").unwrap();
    // Target = first photo of cluster A; other = first photo of cluster B.
    (dir, cat, ids[0], ids[5], cluster_a)
}

fn face_rows(cat: &Catalog, file_id: i64) -> i64 {
    cat.conn()
        .query_row("SELECT COUNT(*) FROM faces WHERE file_id = ?1", [file_id], |r| r.get(0))
        .unwrap()
}

fn person_album_of(cat: &Catalog, file_id: i64) -> Vec<String> {
    cat.conn()
        .prepare(
            "SELECT a.name FROM albums a JOIN album_files af ON af.album_id = a.id \
             WHERE af.file_id = ?1 AND a.person_id IS NOT NULL",
        )
        .unwrap()
        .query_map([file_id], |r| r.get(0))
        .unwrap()
        .flatten()
        .collect()
}

#[test]
fn reset_clears_sidecar_regions_faces_rows_album_membership_and_filter() {
    let (_dir, mut cat, target, other, cluster_a) = seed();
    let path: String = cat
        .conn()
        .query_row("SELECT path FROM files WHERE id = ?1", [target], |r| r.get(0))
        .unwrap();
    let sc_path = sidecar_path(&path);
    assert_eq!(Sidecar::read(&sc_path).unwrap().face_regions().len(), 1);

    let removed = cat.reset_faces(target).unwrap();
    assert_eq!(removed, 1, "one face row removed");

    // Sidecar: no face regions left, but the sidecar itself and its other
    // content survive (the person's name was the only MWG region here).
    let sc = Sidecar::read(&sc_path).unwrap();
    assert!(sc.face_regions().is_empty());
    assert_eq!(sc.rating(), None, "nothing else written");

    // Catalog: face rows and embeddings gone.
    assert_eq!(face_rows(&cat, target), 0);
    let emb_left: i64 = cat
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM embeddings e WHERE NOT EXISTS \
             (SELECT 1 FROM faces f WHERE f.embedding_id = e.embedding_id)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(emb_left, 0, "orphaned embeddings cleaned up");

    // Derived data: out of the person album, out of FTS, out of the filter.
    assert_eq!(person_album_of(&cat, target), Vec::<String>::new());
    assert!(!cat.search_files("Anna", 100).unwrap().contains(&target));
    let hits: Vec<i64> = cat
        .query_files(&FileQuery { has_faces: true, ..FileQuery::default() }, 100)
        .unwrap()
        .iter()
        .map(|f| f.id)
        .collect();
    assert!(!hits.contains(&target));

    // Everything else untouched: the other photo keeps its unnamed face and
    // its detection row; the person itself survives (other photos still
    // carry the name).
    assert_eq!(face_rows(&cat, other), 1);
    assert!(cat.cluster_person(cluster_a).unwrap().is_some());
}

/// Resetting an unknown file id is an error, not a silent no-op.
#[test]
fn reset_of_an_unknown_file_is_an_error() {
    let (_dir, mut cat, _a, _b, _c) = seed();
    assert!(matches!(
        cat.reset_faces(424_242),
        Err(fotobase_catalog::SyncError::UnknownFile(424_242))
    ));
}

/// A Regions structure we do not understand is never rewritten: the reset
/// refuses (sidecar and catalog unchanged) instead of destroying unknown
/// XMP content (docs/sidecar-merge-on-write.md §1).
#[test]
fn reset_of_an_unrecognised_regions_structure_is_refused() {
    let (_dir, mut cat, target, _other, _cluster) = seed();
    let path: String = cat
        .conn()
        .query_row("SELECT path FROM files WHERE id = ?1", [target], |r| r.get(0))
        .unwrap();
    let sc_path = sidecar_path(&path);
    // Corrupt the structure after the fact: drop the RegionList wrapper.
    let raw = std::fs::read_to_string(&sc_path).unwrap();
    let odd = raw
        .replace("<mwg-rs:RegionList>", "<mwg-rs:Other>")
        .replace("</mwg-rs:RegionList>", "</mwg-rs:Other>");
    std::fs::write(&sc_path, odd).unwrap();

    assert!(cat.reset_faces(target).is_err());
    assert!(std::fs::read_to_string(&sc_path).unwrap().contains("mwg-rs:Other"));
    assert_eq!(face_rows(&cat, target), 1, "catalog untouched");
}

/// A photo without any faces resets cleanly (no sidecar created, no rows).
#[test]
fn reset_of_a_faceless_photo_is_a_clean_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    let file_id = add_photo(&mut cat, dir.path(), "lonely.jpg");
    assert_eq!(cat.reset_faces(file_id).unwrap(), 0);
    let path: String = cat
        .conn()
        .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))
        .unwrap();
    assert!(!sidecar_path(&path).exists(), "no sidecar invented");
}
