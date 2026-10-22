//! TASK-01-021 integration: the file-menu / folder-context backend half.
//!
//! "Delete from disk" trashes the file (never a permanent delete) and drops
//! the catalog row. "Refresh thumbnails" invalidates the folder's cache
//! entries and re-queues their renders through the bounded job queue at
//! background priority (PLAN §3 rule 4). "Locate on disk" and "Exit" are
//! thin OS adapters — covered by the per-OS smoke E2E, not here.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use fotobase_catalog::Catalog;
use fotobase_jobs::Queue;
use fotobase_lib::file_menu;
use fotobase_scanner::scan_folder;

fn write_jpg(path: &Path, len: usize) {
    fs::write(path, vec![0xff; len]).unwrap();
}

fn normalize(path: &Path) -> String {
    // Native-separator path, like the scanner stores them (Windows-safe).
    path.components().collect::<PathBuf>().to_string_lossy().into_owned()
}

/// Seed a folder with two photos, scanned into a fresh catalog.
fn fixture() -> (tempfile::TempDir, tempfile::TempDir, Catalog) {
    let lib = tempfile::tempdir().unwrap();
    write_jpg(&lib.path().join("a.jpg"), 10);
    write_jpg(&lib.path().join("b.jpg"), 20);
    let db = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(db.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, lib.path(), &AtomicBool::new(false)).unwrap();
    (lib, db, cat)
}

fn count_files(cat: &Catalog) -> i64 {
    cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap()
}

#[test]
fn remove_file_drops_the_row_and_its_metadata() {
    let (_lib, _db, mut cat) = fixture();
    let files = cat.list_files(usize::MAX).unwrap();
    assert_eq!(files.len(), 2);

    let doomed = files[0].id;
    assert!(cat.remove_file(doomed).unwrap(), "the row must exist before the delete");
    assert_eq!(count_files(&cat), 1, "exactly one row dropped");
    assert!(cat.photo_info(doomed).unwrap().is_none(), "info gone with the row");
    assert!(!cat.remove_file(doomed).unwrap(), "second delete reports nothing removed");
}

#[test]
fn remove_file_cleans_the_fts_index() {
    let (_lib, _db, mut cat) = fixture();
    let files = cat.list_files(usize::MAX).unwrap();
    let doomed = files.iter().find(|f| f.filename == "a.jpg").unwrap();
    cat.remove_file(doomed.id).unwrap();

    let hits = cat.search_files("a.jpg", 10).unwrap();
    assert!(hits.is_empty(), "deleted file must vanish from search results");
}

#[tokio::test]
async fn refresh_folder_thumbnails_invalidates_cache_and_requeues_renders() {
    let (lib, _db, cat) = fixture();
    let cache = tempfile::tempdir().unwrap();
    let queue = Queue::new(8);

    // The scanner leaves `hash` NULL (content hashes are computed lazily);
    // fill them in the way the import pipeline does, so the folder's files
    // are cache-keyed like real cataloged photos.
    let files = cat.list_files(usize::MAX).unwrap();
    for file in &files {
        let path = lib.path().join(&file.filename);
        cat.conn()
            .execute(
                "UPDATE files SET hash = ?1 WHERE id = ?2",
                rusqlite::params![fotobase_scanner::content_hash(&path).unwrap(), file.id],
            )
            .unwrap();
    }

    // Pre-seed the cache with one entry per cataloged hash, as the render
    // pipeline would have written them.
    let hashes: Vec<String> =
        cat.list_files(usize::MAX).unwrap().into_iter().map(|f| f.hash.unwrap()).collect();
    assert_eq!(hashes.len(), 2);
    for hash in &hashes {
        fs::write(cache.path().join(format!("{hash}.jpg")), b"stale").unwrap();
    }

    let requeued =
        file_menu::refresh_folder_thumbnails(&cat, cache.path(), &queue, &normalize(lib.path()))
            .expect("refresh must succeed");

    assert_eq!(requeued, 2, "one render job per cataloged photo in the folder");
    for hash in &hashes {
        assert!(
            !cache.path().join(format!("{hash}.jpg")).exists(),
            "stale cache entry must be invalidated before the re-render"
        );
    }
}

#[tokio::test]
async fn refresh_folder_thumbnails_ignores_unknown_folder() {
    let (_lib, _db, cat) = fixture();
    let cache = tempfile::tempdir().unwrap();
    let queue = Queue::new(8);
    let requeued =
        file_menu::refresh_folder_thumbnails(&cat, cache.path(), &queue, "/not/in/catalog");
    assert_eq!(requeued.unwrap(), 0, "unknown folder re-queues nothing");
}

#[test]
fn delete_to_trash_removes_file_and_catalog_row() {
    let (lib, _db, mut cat) = fixture();
    // A real file on disk (the fixture's a.jpg) plus its catalog row id.
    let files = cat.list_files(usize::MAX).unwrap();
    let doomed = files.iter().find(|f| f.filename == "a.jpg").unwrap();
    let (id, path) = (doomed.id, normalize(&lib.path().join("a.jpg")));
    assert!(Path::new(&path).exists());

    file_menu::delete_to_trash(id, &path, &mut cat).expect("delete must succeed");

    assert!(!Path::new(&path).exists(), "the file must have left its path (trash)");
    assert_eq!(count_files(&cat), 1, "the catalog row must be dropped");
}

#[test]
fn delete_to_trash_missing_file_is_an_error_and_keeps_the_row() {
    let (_lib, _db, mut cat) = fixture();
    let files = cat.list_files(usize::MAX).unwrap();
    let id = files[0].id;

    let result = file_menu::delete_to_trash(id, "/definitely/not/there.jpg", &mut cat);

    assert!(result.is_err(), "a missing file must surface as an error");
    assert_eq!(count_files(&cat), 2, "a failed trash must not drop the row");
}
