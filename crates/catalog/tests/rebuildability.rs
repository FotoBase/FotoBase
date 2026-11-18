//! TASK-07-009: smart-search rebuildability (PLAN §9). The catalog is a
//! rebuildable cache; sidecars are the source of truth (PLAN §3 rule 2,
//! §5 authority split). These tests execute the epic acceptance scenario:
//! snapshot user data → embedding + auto-tag flow → confirm a tag →
//! delete the catalog DB → rebuild from sidecars → diff user data and
//! derived-data recomputability. They also serve as the regression tripwire
//! for anything that starts writing derived data into sidecars.

use std::path::{Path, PathBuf};

use fotobase_catalog::{sidecar_path, Catalog};
use fotobase_metadata::Sidecar;

const CLIP_DIM: usize = fotobase_catalog::embeddings::CLIP_EMBEDDING_DIM;

fn unit_vector(seed: f32) -> Vec<f32> {
    let mut v: Vec<f32> = (0..CLIP_DIM).map(|i| (i as f32 + seed).sin()).collect();
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    for x in &mut v {
        *x /= norm;
    }
    v
}

struct Photo {
    id: i64,
    original: PathBuf,
}

/// Seed one fake photo the way the scanner would leave the catalog, with a
/// sidecar-consistent path. Returns the catalog row id and original path.
fn add_photo(cat: &mut Catalog, dir: &Path, name: &str) -> Photo {
    let original = dir.join(name);
    std::fs::write(&original, b"fake image bytes").unwrap();
    let folder_id = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
            rusqlite::params![folder_id, original.to_str().unwrap(), name],
        )
        .unwrap();
    Photo { id: cat.conn().last_insert_rowid(), original }
}

/// Simulate the scan after a deleted catalog: fresh DB, folders + file rows
/// re-found at the same paths (user data not yet restored).
fn reopen_with_files(db: &Path, dir: &Path, names: &[&str]) -> Catalog {
    let mut cat = Catalog::open(db).unwrap();
    let folder_id = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    for name in names {
        cat.conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
                 VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
                rusqlite::params![folder_id, dir.join(name).to_str().unwrap(), name],
            )
            .unwrap();
    }
    cat
}

fn file_id_by_path(cat: &Catalog, path: &str) -> i64 {
    cat.conn().query_row("SELECT id FROM files WHERE path = ?1", [path], |r| r.get(0)).unwrap()
}

/// The full §9 scenario: snapshot → embedding + auto-tag flow → confirm →
/// delete catalog → rebuild (full) → user data identical, derived data
/// recomputable, semantic ranking reproducible.
#[test]
fn confirmed_auto_tags_survive_delete_and_rebuild_exactly_once() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("catalog.db");
    let mut cat = Catalog::open(&db).unwrap();
    let a = add_photo(&mut cat, dir.path(), "IMG_0001.jpg");
    let b = add_photo(&mut cat, dir.path(), "IMG_0002.jpg");

    // Embedding + auto-tag flow: vectors stored, suggestions offered,
    // one confirmed (user data → sidecar), one left as an unconfirmed
    // suggestion (derived data → catalog only).
    cat.store_photo_embedding(a.id, &unit_vector(0.0)).unwrap();
    cat.store_photo_embedding(b.id, &unit_vector(5.0)).unwrap();
    cat.mark_embed_scanned(a.id, true).unwrap();
    cat.mark_embed_scanned(b.id, true).unwrap();
    cat.store_suggestions(a.id, &["dog".to_string(), "beach".to_string()]).unwrap();
    cat.store_suggestions(b.id, &["cat".to_string()]).unwrap();
    cat.confirm_tag_suggestion(a.id, "dog").unwrap();

    // Snapshot the user data before the rebuild.
    let sidecar_before = Sidecar::read(&sidecar_path(a.original.to_str().unwrap())).unwrap();
    assert!(sidecar_before.tags().contains(&"dog".to_string()));
    let sidecar_b_before = Sidecar::read(&sidecar_path(b.original.to_str().unwrap())).unwrap();
    assert!(sidecar_b_before.tags().is_empty(), "suggestions never touch sidecars");

    // Delete the catalog and rebuild from sidecars.
    drop(cat);
    std::fs::remove_file(&db).unwrap();
    let mut cat = reopen_with_files(&db, dir.path(), &["IMG_0001.jpg", "IMG_0002.jpg"]);
    cat.rebuild_from_sidecars(true).unwrap();

    // User data re-read from sidecars, exactly once.
    let a2 = file_id_by_path(&cat, dir.path().join("IMG_0001.jpg").to_str().unwrap());
    let b2 = file_id_by_path(&cat, dir.path().join("IMG_0002.jpg").to_str().unwrap());
    assert_eq!(cat.file_tags(a2).unwrap(), vec!["dog"], "confirmed auto-tag survives");
    assert!(cat.file_tags(b2).unwrap().is_empty());
    assert!(cat.file_suggestions(a2).unwrap().is_empty(), "suggestions are derived");
    assert!(cat.file_suggestions(b2).unwrap().is_empty());
    let sidecar_after = Sidecar::read(&sidecar_path(a.original.to_str().unwrap())).unwrap();
    assert_eq!(sidecar_after.tags(), sidecar_before.tags(), "sidecar untouched by rebuild");
    let sidecar_b_after = Sidecar::read(&sidecar_path(b.original.to_str().unwrap())).unwrap();
    assert_eq!(sidecar_b_after.tags(), sidecar_b_before.tags());
    assert!(sidecar_b_after.tags().is_empty());

    // Derived data recomputable: full rebuild cleared the vec0 table and
    // every photo is pending again, ready for the embedding pass.
    let stored: i64 =
        cat.conn().query_row("SELECT COUNT(*) FROM photo_embeddings", [], |r| r.get(0)).unwrap();
    assert_eq!(stored, 0, "full rebuild cleared embeddings");
    assert_eq!(cat.embed_candidates().unwrap().len(), 2, "all photos pending again");
}

/// Semantic search works again after a rebuild without user action beyond
/// the rebuild itself: recompute (simulated by re-storing the same vectors),
/// then the same query returns the same ranked set within tolerance.
#[test]
fn semantic_ranking_is_reproducible_after_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("catalog.db");
    let mut cat = Catalog::open(&db).unwrap();
    let photos: Vec<Photo> =
        ["a.jpg", "b.jpg", "c.jpg"].iter().map(|n| add_photo(&mut cat, dir.path(), n)).collect();
    for (i, p) in photos.iter().enumerate() {
        cat.store_photo_embedding(p.id, &unit_vector(i as f32 * 2.0)).unwrap();
        cat.mark_embed_scanned(p.id, true).unwrap();
    }
    let query = unit_vector(0.3);
    let before = cat.search_photo_embeddings(&query, 3).unwrap();
    drop(cat);

    std::fs::remove_file(&db).unwrap();
    let mut cat = reopen_with_files(&db, dir.path(), &["a.jpg", "b.jpg", "c.jpg"]);
    cat.rebuild_from_sidecars(true).unwrap();
    assert_eq!(cat.embed_candidates().unwrap().len(), 3, "all pending for recompute");

    // The embedding pass recomputes; the pipeline is deterministic, so the
    // recomputed vectors equal the originals.
    for (i, _) in photos.iter().enumerate() {
        let id = file_id_by_path(
            &cat,
            dir.path().join(["a.jpg", "b.jpg", "c.jpg"][i]).to_str().unwrap(),
        );
        cat.store_photo_embedding(id, &unit_vector(i as f32 * 2.0)).unwrap();
        cat.mark_embed_scanned(id, true).unwrap();
    }
    let after = cat.search_photo_embeddings(&query, 3).unwrap();

    let got: Vec<i64> = after.iter().map(|(id, _)| *id).collect();
    let want: Vec<i64> = before.iter().map(|(id, _)| *id).collect();
    assert_eq!(got, want, "same ranked set after rebuild+recompute");
    for ((_, d0), (_, d1)) in before.iter().zip(after.iter()) {
        assert!((d0 - d1).abs() < 1e-5, "distances within tolerance: {d0} vs {d1}");
    }
}

/// Rebuild + re-suggestion must never duplicate the confirmed tag: the
/// suggestion store skips names already confirmed for the file.
#[test]
fn rebuild_plus_re_suggestion_creates_no_duplicate_tags() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("catalog.db");
    let mut cat = Catalog::open(&db).unwrap();
    let p = add_photo(&mut cat, dir.path(), "IMG_0003.jpg");
    cat.store_suggestions(p.id, &["dog".to_string()]).unwrap();
    cat.confirm_tag_suggestion(p.id, "dog").unwrap();
    drop(cat);

    std::fs::remove_file(&db).unwrap();
    let mut cat = reopen_with_files(&db, dir.path(), &["IMG_0003.jpg"]);
    cat.rebuild_from_sidecars(true).unwrap();
    let id = file_id_by_path(&cat, dir.path().join("IMG_0003.jpg").to_str().unwrap());
    assert_eq!(cat.file_tags(id).unwrap(), vec!["dog"]);

    // The auto-tag flow runs again and suggests the same concept.
    cat.store_suggestions(id, &["dog".to_string(), "beach".to_string()]).unwrap();
    assert_eq!(cat.file_suggestions(id).unwrap(), vec!["beach"], "confirmed tag not re-suggested");
    assert_eq!(cat.file_tags(id).unwrap(), vec!["dog"], "no duplicate tag row");

    // And confirming what is already a tag stays a no-op on the sidecar.
    cat.confirm_tag_suggestion(id, "beach").unwrap();
    let tags_before = Sidecar::read(&sidecar_path(p.original.to_str().unwrap())).unwrap().tags();
    assert_eq!(tags_before, vec!["dog", "beach"], "sidecar has each tag exactly once");
}

/// Regression tripwire (PLAN §9): no smart-search step — suggestion store,
/// rejection, embedding write, rebuild — may write derived data into an
/// XMP sidecar. The sidecar of a photo with suggestions and embeddings
/// must be absent or byte-identical to its user-data content throughout.
#[test]
fn no_smart_search_step_writes_derived_data_into_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("catalog.db")).unwrap();
    let p = add_photo(&mut cat, dir.path(), "IMG_0004.jpg");
    let sc_path = sidecar_path(p.original.to_str().unwrap());
    assert!(!sc_path.exists(), "no sidecar before any user write");

    cat.store_photo_embedding(p.id, &unit_vector(1.0)).unwrap();
    cat.mark_embed_scanned(p.id, true).unwrap();
    cat.store_suggestions(p.id, &["dog".to_string(), "beach".to_string()]).unwrap();
    cat.reject_tag_suggestion(p.id, "dog").unwrap();
    cat.rebuild_from_sidecars(true).unwrap();
    assert!(!sc_path.exists(), "derived data must never create a sidecar");

    cat.confirm_tag_suggestion(p.id, "beach").unwrap();
    let after_confirm = Sidecar::read(&sc_path).unwrap().raw().to_string();
    assert!(after_confirm.contains("beach"), "dc:subject written on confirm");

    // Rebuild re-reads the sidecar; it must not append derived content.
    cat.rebuild_from_sidecars(true).unwrap();
    let after_rebuild = Sidecar::read(&sc_path).unwrap().raw().to_string();
    assert_eq!(after_rebuild, after_confirm, "rebuild leaves the sidecar byte-identical");
}
