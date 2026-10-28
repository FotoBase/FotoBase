//! TASK-03-006: tags write-through to `dc:subject` (+ `lr:hierarchicalSubject`
//! for nested tags), tag rename across sidecars, and rebuild survival.

use std::path::{Path, PathBuf};

use fotobase_catalog::{sidecar_path, Catalog};
use fotobase_metadata::Sidecar;

fn add_photo(cat: &mut Catalog, dir: &Path, name: &str) -> (i64, PathBuf) {
    let original = dir.join(name);
    std::fs::write(&original, b"fake image bytes").unwrap();
    let folder_id = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    let path = original.to_str().unwrap().to_string();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
            rusqlite::params![folder_id, path, name],
        )
        .unwrap();
    (cat.conn().last_insert_rowid(), original)
}

fn catalog_tags(cat: &Catalog, id: i64) -> Vec<String> {
    cat.file_tags(id).unwrap()
}

#[test]
fn add_tag_writes_dc_subject_and_catalog_mirror() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");

    let tags = cat.add_tag(id, "sea").unwrap();

    assert_eq!(tags, vec!["sea"]);
    let sc = Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap();
    assert_eq!(sc.tags(), vec!["sea"]);
    assert_eq!(catalog_tags(&cat, id), vec!["sea"]);
}

#[test]
fn add_tag_twice_does_not_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.add_tag(id, "sea").unwrap();

    let tags = cat.add_tag(id, "sea").unwrap();

    assert_eq!(tags, vec!["sea"]);
}

#[test]
fn remove_tag_clears_both_bags_and_the_mirror() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.add_tag(id, "sea").unwrap();

    let tags = cat.remove_tag(id, "sea").unwrap();

    assert!(tags.is_empty());
    let sc = Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap();
    assert!(sc.tags().is_empty());
    assert!(catalog_tags(&cat, id).is_empty());
}

#[test]
fn nested_tag_lands_in_lr_hierarchical_subject_and_dc_subject() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");

    let tags = cat.add_tag(id, "Vacation|2024").unwrap();

    assert_eq!(tags, vec!["Vacation|2024"]);
    let sc = Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap();
    assert!(sc.tags().contains(&"Vacation|2024".to_string()), "{:?}", sc.tags());
    assert_eq!(sc.hierarchical_tags(), vec!["Vacation|2024"]);
    // Readable by darktable/digiKam: the raw XML carries both fragments.
    let raw = sc.raw().to_string();
    assert!(raw.contains("lr:hierarchicalSubject"), "{raw}");
    assert!(raw.contains("dc:subject"), "{raw}");
}

#[test]
fn rename_tag_rewrites_every_affected_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let mut originals = Vec::new();
    let mut ids = Vec::new();
    for name in ["a.jpg", "b.jpg", "c.jpg"] {
        let (id, original) = add_photo(&mut cat, dir.path(), name);
        cat.add_tag(id, "sea").unwrap();
        ids.push(id);
        originals.push(original);
    }

    let renamed = cat.rename_tag("sea", "ocean").unwrap();

    assert_eq!(renamed, 3);
    for original in &originals {
        let sc = Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap();
        assert!(sc.tags().contains(&"ocean".to_string()), "{:?}", sc.tags());
        assert!(!sc.tags().contains(&"sea".to_string()), "{:?}", sc.tags());
    }
    for id in &ids {
        assert_eq!(catalog_tags(&cat, *id), vec!["ocean"]);
    }
    // The stale tag name is gone from the derived cache.
    assert!(cat.all_tags().unwrap().iter().all(|(n, _)| n != "sea"));
}

#[test]
fn rename_tag_rewrites_nested_path_segments() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.add_tag(id, "Vacation|2024").unwrap();

    cat.rename_tag("Vacation", "Trips").unwrap();

    let sc = Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap();
    assert_eq!(sc.hierarchical_tags(), vec!["Trips|2024"]);
    assert!(sc.tags().contains(&"Trips|2024".to_string()));
}

#[test]
fn tags_survive_catalog_rebuild_from_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.add_tag(id, "sea").unwrap();
    cat.add_tag(id, "Trip|2026").unwrap();

    // Wipe the derived cache (simulating a deleted/rebuilt catalog mirror)
    // and rebuild from the sidecars.
    cat.conn().execute("DELETE FROM file_tags", []).unwrap();
    cat.conn().execute("DELETE FROM tags", []).unwrap();
    cat.rebuild_from_sidecars(true).unwrap();

    let mut tags = catalog_tags(&cat, id);
    tags.sort();
    assert_eq!(tags, vec!["Trip|2026", "sea"]);
}

#[test]
fn originals_are_never_touched_by_tag_writes() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let before = std::fs::read(&original).unwrap();

    cat.add_tag(id, "sea").unwrap();
    cat.rename_tag("sea", "ocean").unwrap();

    assert_eq!(std::fs::read(&original).unwrap(), before);
}

#[test]
fn all_tags_lists_names_with_usage_counts() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (a, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (b, _) = add_photo(&mut cat, dir.path(), "b.jpg");
    cat.add_tag(a, "sea").unwrap();
    cat.add_tag(b, "sea").unwrap();
    cat.add_tag(b, "mountain").unwrap();

    let tags = cat.all_tags().unwrap();

    assert_eq!(tags, vec![("mountain".to_string(), 1), ("sea".to_string(), 2)]);
}

#[test]
fn unknown_file_is_rejected_without_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let err = cat.add_tag(999, "sea").unwrap_err();
    assert!(matches!(err, fotobase_catalog::SyncError::UnknownFile(999)), "{err:?}");
}
