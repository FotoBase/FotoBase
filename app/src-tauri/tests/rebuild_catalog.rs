//! TASK-01-023 integration: rebuild catalog with progress.
//!
//! "Rebuild catalog" re-scans every watched folder in full mode (the
//! (mtime, size) skip is disabled, PLAN §5) and repopulates derived data.
//! Sidecar-backed user data in the catalog must survive: file ids stay
//! stable across upserts, so `metadata`/`file_tags` rows are never dropped
//! (§9 rebuildability — deleting the catalog must not lose stars, tags,
//! captions). Originals and sidecars are never written.

use std::fs;
use std::path::{Path, PathBuf};

use fotobase_catalog::Catalog;
use fotobase_lib::rebuild::{rebuild, watched_folders, RebuildProgress};
use fotobase_scanner::scan_folder;

fn make_library(root: &Path) {
    fs::create_dir_all(root.join("2024")).unwrap();
    fs::create_dir_all(root.join("2025")).unwrap();
    fs::write(root.join("2024/a.jpg"), vec![0xff; 10]).unwrap();
    fs::write(root.join("2024/b.png"), vec![0xff; 20]).unwrap();
    fs::write(root.join("2025/c.jpg"), vec![0xff; 30]).unwrap();
}

fn open_catalog(dir: &Path) -> Catalog {
    Catalog::open(dir.join("catalog.db")).expect("open catalog")
}

fn normalize(path: &Path) -> String {
    // Native-separator path, like the scanner stores them (Windows-safe).
    path.components().collect::<PathBuf>().to_string_lossy().into_owned()
}

fn user_data(cat: &Catalog, path: &Path) -> (String, i64) {
    cat.conn()
        .query_row(
            "SELECT m.caption, m.rating FROM metadata m
             JOIN files f ON f.id = m.file_id WHERE f.path = ?1",
            [normalize(path)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

#[test]
fn rebuild_preserves_user_data_and_repopulates_derived_rows() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    make_library(lib.path());

    let mut cat = open_catalog(data.path());
    scan_folder(&mut cat, lib.path(), &std::sync::atomic::AtomicBool::new(false)).unwrap();

    // Sidecar-backed user data, seeded the way the metadata layer (03-08)
    // will once it exists: caption + rating on one photo.
    cat.conn()
        .execute(
            "INSERT INTO metadata(file_id, caption, rating)
             SELECT id, 'family trip', 5 FROM files WHERE path = ?1",
            [normalize(&lib.path().join("2024/a.jpg"))],
        )
        .unwrap();

    // Library drift since the last scan: one new photo, one deleted.
    fs::write(lib.path().join("2025/new.jpg"), vec![0xff; 5]).unwrap();
    fs::remove_file(lib.path().join("2024/b.png")).unwrap();

    let roots = watched_folders(&cat);
    // Top-level roots only: a nested row (2024/2025) is covered by its
    // parent's walk, so it must not be re-scanned separately.
    assert_eq!(roots, vec![normalize(lib.path())], "roots: {roots:?}");

    let progress: std::sync::Mutex<Vec<RebuildProgress>> = std::sync::Mutex::new(Vec::new());
    let report =
        rebuild(&mut cat, &roots, &|p| progress.lock().unwrap().push(p)).expect("rebuild succeeds");
    let progress = progress.into_inner().unwrap();

    assert_eq!(report.added, 1, "new.jpg is picked up: {report:?}");
    assert_eq!(report.removed, 1, "deleted b.png is pruned");
    assert!(!report.cancelled);

    // §9 rebuildability: user data identical after the rebuild.
    assert_eq!(user_data(&cat, &lib.path().join("2024/a.jpg")), ("family trip".into(), 5));

    // Derived rows match the library on disk exactly.
    let count: i64 = cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 3);

    // Progress: every watched folder is reported as done, files accumulate.
    assert_eq!(progress.last().unwrap().folders_done, roots.len());
    assert_eq!(progress.last().unwrap().folders_total, roots.len());
    assert_eq!(progress.last().unwrap().files_done, 3);
    for pair in progress.windows(2) {
        assert!(pair[0].files_done <= pair[1].files_done, "monotonic: {progress:?}");
        assert!(pair[0].folders_done <= pair[1].folders_done);
    }
}

#[test]
fn rebuild_handles_a_vanished_watch_root_gracefully() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    make_library(lib.path());

    let mut cat = open_catalog(data.path());
    scan_folder(&mut cat, lib.path(), &std::sync::atomic::AtomicBool::new(false)).unwrap();

    let gone = lib.path().join("removed-dir");
    fs::create_dir_all(&gone).unwrap();
    let roots = vec![normalize(&gone)];

    let progress: std::sync::Mutex<Vec<RebuildProgress>> = std::sync::Mutex::new(Vec::new());
    let report = rebuild(&mut cat, &roots, &|p| progress.lock().unwrap().push(p))
        .expect("a vanished root must not fail the rebuild");
    assert_eq!(report.added + report.updated + report.removed, 0);
    let progress = progress.into_inner().unwrap();
    let last = progress.last().expect("final progress is still emitted");
    assert_eq!((last.folders_done, last.folders_total), (1, 1));
}
