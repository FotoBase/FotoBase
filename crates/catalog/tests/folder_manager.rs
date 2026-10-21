//! Folder Manager data tests (TASK-01-016, PLAN §2A): every catalog folder
//! carries a scan mode (`once` = scanned, not watched; `always` = watched),
//! the mode can be changed per folder, and removing a folder drops its
//! catalog rows only — files on disk are the app's business, never the
//! catalog's.

use fotobase_catalog::{Catalog, ScanMode};
use rusqlite::params;
use tempfile::TempDir;

fn open_in(dir: &TempDir) -> Catalog {
    Catalog::open(dir.path().join("catalog.db")).expect("open catalog")
}

/// Insert a file row directly (scanner-shaped).
fn add_file(cat: &mut Catalog, folder: i64, path: &str) {
    let filename = path.rsplit('/').next().unwrap().to_string();
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, hash, taken_at, media_type)
             VALUES (?1, ?2, ?3, 1, 1.0, NULL, NULL, 'image')",
            params![folder, path, filename],
        )
        .unwrap();
}

#[test]
fn folders_default_to_scan_once_and_expose_their_mode() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    cat.seed_folder("/photos/trip").unwrap();

    let folders = cat.list_folders().unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].path, "/photos/trip");
    assert_eq!(folders[0].scan_mode, ScanMode::Once);
}

#[test]
fn scan_mode_can_be_set_and_persists() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    cat.seed_folder("/photos/trip").unwrap();

    assert!(cat.set_folder_scan_mode("/photos/trip", ScanMode::Always).unwrap());
    let folders = cat.list_folders().unwrap();
    assert_eq!(folders[0].scan_mode, ScanMode::Always);

    // Back to once (un-watching).
    assert!(cat.set_folder_scan_mode("/photos/trip", ScanMode::Once).unwrap());
    assert_eq!(cat.list_folders().unwrap()[0].scan_mode, ScanMode::Once);
}

#[test]
fn setting_the_mode_of_an_unknown_folder_reports_no_row() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    assert!(!cat.set_folder_scan_mode("/photos/ghost", ScanMode::Always).unwrap());
}

#[test]
fn watched_folder_paths_lists_only_scan_always_roots() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    cat.seed_folder("/photos/a").unwrap();
    cat.seed_folder("/photos/b").unwrap();
    cat.seed_folder("/photos/c").unwrap();
    cat.set_folder_scan_mode("/photos/b", ScanMode::Always).unwrap();

    let watched = cat.watched_folder_paths().unwrap();
    assert_eq!(watched, vec!["/photos/b".to_string()]);
}

#[test]
fn removing_a_folder_drops_its_tree_and_cascades_files() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let root = cat.seed_folder("/photos").unwrap();
    let child = cat.seed_folder("/photos/2023").unwrap();
    let other = cat.seed_folder("/videos").unwrap();
    add_file(&mut cat, root, "/photos/a.jpg");
    add_file(&mut cat, child, "/photos/2023/b.jpg");
    add_file(&mut cat, other, "/videos/c.jpg");

    // Removing "/photos" removes the folder AND its nested rows in one go.
    let removed = cat.remove_folder_tree("/photos").unwrap();
    assert_eq!(removed, 2);

    let paths: Vec<String> = cat.list_folders().unwrap().into_iter().map(|f| f.path).collect();
    assert_eq!(paths, vec!["/videos".to_string()]);

    // Cascaded: no file rows (and no FTS rows) survive for the removed tree.
    assert_eq!(cat.list_files(usize::MAX).unwrap().len(), 1);
    assert!(cat.search_files("a.jpg", 10).unwrap().is_empty());
    assert_eq!(cat.search_files("c.jpg", 10).unwrap().len(), 1);
}

#[test]
fn removing_an_unknown_folder_reports_no_row() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    assert_eq!(cat.remove_folder_tree("/photos/ghost").unwrap(), 0);
}

#[test]
fn scan_mode_survives_reopening_the_catalog() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("catalog.db");
    {
        let mut cat = Catalog::open(&path).unwrap();
        cat.seed_folder("/photos").unwrap();
        cat.set_folder_scan_mode("/photos", ScanMode::Always).unwrap();
    }
    let cat = Catalog::open(&path).unwrap();
    assert_eq!(cat.list_folders().unwrap()[0].scan_mode, ScanMode::Always);
}

#[test]
fn scan_mode_serializes_as_the_stored_string() {
    assert_eq!(serde_json::to_string(&ScanMode::Once).unwrap(), r#""once""#);
    assert_eq!(serde_json::to_string(&ScanMode::Always).unwrap(), r#""always""#);
}
