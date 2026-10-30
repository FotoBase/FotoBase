//! TASK-03-024 integration: two catalogs never contaminate each other.
//!
//! Fixture catalogs A and B: star a photo in A, "switch" (close A's
//! connection, reopen B), assert B's FTS / scan state shows nothing from A,
//! then switch back and assert the star persisted in A. The Folder Manager
//! opened against B must have none of A's watched folders (watcher state is
//! per-catalog, per the technical spec).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use fotobase_catalog::Catalog;
use fotobase_lib::catalog_registry::Registry;

/// Insert one photo row the way the scanner would (rating.rs test helper).
fn add_photo(cat: &mut Catalog, dir: &Path, name: &str) -> i64 {
    let original = dir.join(name);
    fs::write(&original, b"bytes").unwrap();
    let folder = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, ?2, ?3, 5, 0.0, 'image')",
            rusqlite::params![folder, original.to_str().unwrap(), name],
        )
        .unwrap();
    cat.conn().last_insert_rowid()
}

fn db_path(dir: &Path) -> PathBuf {
    dir.join("catalog.db")
}

#[test]
fn star_in_a_survives_switch_to_b_and_back() {
    let tmp = tempfile::tempdir().unwrap();
    let dir_a = tmp.path().join("A");
    let dir_b = tmp.path().join("B");
    fs::create_dir_all(&dir_a).unwrap();
    fs::create_dir_all(&dir_b).unwrap();

    let mut registry = Registry::load(tmp.path());
    registry.create("A", &dir_a).unwrap();
    registry.create("B", &dir_b).unwrap();
    registry.switch("A").unwrap();
    assert_eq!(registry.last_used.as_deref(), Some("A"));

    // Catalog A: one photo, starred.
    let file_id = {
        let mut cat = Catalog::open(db_path(&dir_a)).unwrap();
        let id = add_photo(&mut cat, &dir_a, "a.jpg");
        cat.set_rating(id, 5).unwrap();
        id
    }; // drop = close = WAL checkpoint

    // Switch to B: registry tracks it; B's catalog is empty.
    registry.switch("B").unwrap();
    assert_eq!(registry.last_used.as_deref(), Some("B"));
    {
        let mut cat_b = Catalog::open(db_path(&dir_b)).unwrap();
        assert!(cat_b.list_files(100).unwrap().is_empty(), "B must not see A's files");
        assert!(
            cat_b.search_files("a.jpg", 10).unwrap().is_empty(),
            "B's FTS must match nothing from A"
        );
        assert!(
            cat_b.watched_folder_paths().unwrap().is_empty(),
            "B must have none of A's watched folders"
        );
        // B's scan state is its own: scanning B finds only B's files.
        let report =
            fotobase_scanner::scan_folder(&mut cat_b, &dir_b, &AtomicBool::new(false)).unwrap();
        assert_eq!(report.added, 0, "B's scan must not ingest anything from A");
    }

    // Switch back to A: the star persisted across the round trip.
    registry.switch("A").unwrap();
    let cat_a = Catalog::open(db_path(&dir_a)).unwrap();
    assert_eq!(cat_a.list_files(100).unwrap().len(), 1, "A keeps its photo");
    let starred = cat_a.list_starred(None, 5, 10).unwrap();
    assert_eq!(starred.len(), 1, "the star persisted");
    assert_eq!(starred[0].id, file_id);
}

#[test]
fn folder_manager_opened_on_b_has_empty_watcher_state() {
    let tmp = tempfile::tempdir().unwrap();
    let dir_a = tmp.path().join("A");
    let dir_b = tmp.path().join("B");
    fs::create_dir_all(&dir_a).unwrap();
    fs::create_dir_all(&dir_b).unwrap();

    // A watches a folder (scan mode 'always' persisted in A's catalog).
    {
        let mut cat_a = Catalog::open(db_path(&dir_a)).unwrap();
        fotobase_scanner::scan_folder(&mut cat_a, &dir_a, &AtomicBool::new(false)).unwrap();
        cat_a
            .set_folder_scan_mode(dir_a.to_str().unwrap(), fotobase_catalog::ScanMode::Always)
            .unwrap();
    }

    // Switch lifecycle: A's manager is stopped, B's manager opens B's DB.
    let fm_b = fotobase_lib::folder_manager::FolderManager::open_with(
        &db_path(&dir_b),
        std::time::Duration::from_millis(300),
        None,
    )
    .unwrap();
    assert!(!fm_b.has_watched_folders(), "watcher state must not leak from A into B");
    fm_b.stop();
}

#[test]
fn catalogs_are_on_disk_separate_databases() {
    let tmp = tempfile::tempdir().unwrap();
    let mut registry = Registry::load(tmp.path());
    registry.create("Personal", &tmp.path().join("personal")).unwrap();
    let a = registry.get("Default").unwrap();
    let b = registry.get("Personal").unwrap();
    assert_ne!(a.dir, b.dir, "each catalog has its own directory");
    assert_ne!(
        fotobase_lib::catalog_db_path(&a.dir),
        fotobase_lib::catalog_db_path(&b.dir),
        "each catalog has its own database"
    );
    assert_ne!(
        fotobase_lib::catalog_cache_dir(&a.dir),
        fotobase_lib::catalog_cache_dir(&b.dir),
        "each catalog has its own thumbnail cache"
    );
}
