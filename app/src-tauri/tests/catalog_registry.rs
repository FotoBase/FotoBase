//! TASK-03-024 unit tests: the catalog registry (create / switch / last-used).
//!
//! The registry is a small JSON list of catalog locations under the app data
//! dir, separate from any catalog (technical spec). The default entry points
//! at the data dir itself so existing single-catalog installs keep their
//! layout (`data_dir/catalog.db` + `data_dir/thumbnails`) with no migration.

use std::path::Path;

use fotobase_lib::catalog_registry::{Registry, RegistryError};

#[test]
fn default_registry_has_one_default_catalog_at_data_dir() {
    let dir = tempfile::tempdir().unwrap();
    let r = Registry::load(dir.path());
    assert_eq!(r.catalogs.len(), 1);
    assert_eq!(r.catalogs[0].name, "Default");
    assert_eq!(r.catalogs[0].dir, dir.path());
    assert_eq!(r.last_used.as_deref(), Some("Default"));
}

#[test]
fn corrupt_file_falls_back_to_default_registry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("catalogs.json"), "{ not json").unwrap();
    let r = Registry::load(dir.path());
    assert_eq!(r.catalogs.len(), 1);
    assert_eq!(r.catalogs[0].name, "Default");
}

#[test]
fn create_adds_entry_and_makes_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Registry::load(dir.path());
    let work = dir.path().join("catalogs").join("work");
    r.create("Work", &work).unwrap();
    assert!(work.is_dir(), "create must make the per-catalog directory");
    assert_eq!(r.catalogs.len(), 2);
    assert!(r.get("Work").is_some());
}

#[test]
fn duplicate_names_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Registry::load(dir.path());
    assert!(matches!(
        r.create("Default", &dir.path().join("x")),
        Err(RegistryError::DuplicateName(_))
    ));
    // Names are unique case-insensitively ("Work" vs "work").
    r.create("Work", &dir.path().join("a")).unwrap();
    assert!(matches!(
        r.create("work", &dir.path().join("b")),
        Err(RegistryError::DuplicateName(_))
    ));
}

#[test]
fn empty_name_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Registry::load(dir.path());
    assert!(matches!(r.create("  ", &dir.path().join("x")), Err(RegistryError::EmptyName)));
}

#[test]
fn switch_sets_last_used() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Registry::load(dir.path());
    r.create("Work", &dir.path().join("work")).unwrap();
    r.switch("Work").unwrap();
    assert_eq!(r.last_used.as_deref(), Some("Work"));
    r.switch("Default").unwrap();
    assert_eq!(r.last_used.as_deref(), Some("Default"));
}

#[test]
fn switch_unknown_name_errors_and_leaves_last_used() {
    let dir = tempfile::tempdir().unwrap();
    let mut r = Registry::load(dir.path());
    assert!(matches!(r.switch("Nope"), Err(RegistryError::UnknownCatalog(_))));
    assert_eq!(r.last_used.as_deref(), Some("Default"));
}

#[test]
fn round_trips_through_disk() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    {
        let mut r = Registry::load(dir.path());
        r.create("Work", &work).unwrap();
        r.switch("Work").unwrap();
        r.save(dir.path()).unwrap();
    }
    let r = Registry::load(dir.path());
    assert_eq!(r.catalogs.len(), 2);
    assert_eq!(r.last_used.as_deref(), Some("Work"));
    let entry = r.get("Work").unwrap();
    assert_eq!(entry.dir, work);
}

#[test]
fn last_used_falls_back_to_first_catalog_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("catalogs.json"),
        r#"{"catalogs":[{"name":"A","dir":"/tmp/a"},{"name":"B","dir":"/tmp/b"}],"last_used":"Gone"}"#,
    )
    .unwrap();
    let r = Registry::load(dir.path());
    assert_eq!(r.last_used.as_deref(), Some("A"), "dangling last_used must resolve");
}

#[test]
fn save_is_atomic_tmp_then_rename() {
    let dir = tempfile::tempdir().unwrap();
    let r = Registry::load(dir.path());
    r.save(dir.path()).unwrap();
    // The `.tmp` sibling must not survive a successful save.
    assert!(!dir.path().join("catalogs.json.tmp").exists());
    assert!(dir.path().join("catalogs.json").exists());
}

#[test]
fn default_paths_follow_the_entry_dir() {
    let dir = tempfile::tempdir().unwrap();
    let r = Registry::load(dir.path());
    let entry = r.get("Default").unwrap();
    assert_eq!(fotobase_lib::catalog_db_path(&entry.dir), dir.path().join("catalog.db"));
    assert_eq!(fotobase_lib::catalog_cache_dir(&entry.dir), dir.path().join("thumbnails"));
    let _ = Path::new(""); // silence unused-import churn if the API grows
}
