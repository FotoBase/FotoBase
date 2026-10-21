//! Scanner integration tests (TASK-01-006, TDD).
//!
//! Walk, mtime+size change detection, initial scan, incremental rescan.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use fotobase_catalog::Catalog;
use fotobase_scanner::scan_folder;

fn open_catalog(dir: &Path) -> Catalog {
    Catalog::open(dir.join("catalog.db")).expect("open catalog")
}

fn count_files(cat: &Catalog) -> i64 {
    cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap()
}

fn count_folders(cat: &Catalog) -> i64 {
    cat.conn().query_row("SELECT COUNT(*) FROM folders", [], |r| r.get(0)).unwrap()
}

fn file_row(cat: &Catalog, path: &Path) -> (i64, f64, String) {
    // Normalize separators: `root.join("2024/summer/c.jpg")` yields mixed
    // separators on Windows, while the scanner stores native-separator paths.
    let path = path.components().collect::<PathBuf>();
    cat.conn()
        .query_row(
            "SELECT size, mtime, media_type FROM files WHERE path = ?1",
            [path.to_string_lossy().as_ref()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}

fn write_jpg(path: &Path, len: usize) {
    fs::write(path, vec![0xff; len]).unwrap();
}

/// Build a small library: nested dirs, images, a video, and non-media files.
fn make_library(root: &Path) {
    fs::create_dir_all(root.join("2024/summer")).unwrap();
    fs::create_dir_all(root.join("2025")).unwrap();
    write_jpg(&root.join("a.jpg"), 10);
    write_jpg(&root.join("2024/b.png"), 20);
    write_jpg(&root.join("2024/summer/c.jpg"), 30);
    write_jpg(&root.join("2025/d.heic"), 40);
    fs::write(root.join("2025/e.mp4"), vec![0; 50]).unwrap();
    // Non-media files are never catalogued.
    fs::write(root.join("notes.txt"), "not a photo").unwrap();
    fs::write(root.join("2024/xmp-ok.jpg.xmp"), "sidecar").unwrap();
}

#[test]
fn initial_scan_populates_catalog() {
    let tmp = tempfile::tempdir().unwrap();
    make_library(tmp.path());

    let mut cat = open_catalog(tmp.path());
    let report = scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();

    // 4 images + 1 video; .txt and .xmp are not media.
    assert_eq!(report.added, 5, "report: {report:?}");
    assert_eq!(report.updated, 0);
    assert_eq!(report.skipped, 0);
    assert_eq!(report.removed, 0);
    assert_eq!(count_files(&cat), 5);
    // root, 2024, 2024/summer, 2025
    assert_eq!(count_folders(&cat), 4);

    let (size, _mtime, media) = file_row(&cat, &tmp.path().join("2024/summer/c.jpg"));
    assert_eq!(size, 30);
    assert_eq!(media, "image");
    let (_, _, media) = file_row(&cat, &tmp.path().join("2025/e.mp4"));
    assert_eq!(media, "video");
}

#[test]
fn rescan_without_changes_is_noop() {
    let tmp = tempfile::tempdir().unwrap();
    make_library(tmp.path());

    let mut cat = open_catalog(tmp.path());
    scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();

    let report = scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.added, 0);
    assert_eq!(report.updated, 0);
    assert_eq!(report.skipped, 5, "no file re-reads: {report:?}");
    assert_eq!(report.removed, 0);
    assert_eq!(count_files(&cat), 5);
}

#[test]
fn incremental_detects_add_modify_touch_delete() {
    let tmp = tempfile::tempdir().unwrap();
    make_library(tmp.path());

    let mut cat = open_catalog(tmp.path());
    scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();

    // touch: same size, new mtime
    let touched = tmp.path().join("a.jpg");
    let new_mtime = SystemTime::now() + Duration::from_secs(3600);
    filetime::set_file_mtime(&touched, filetime::FileTime::from_system_time(new_mtime)).unwrap();

    // modify: different size
    write_jpg(&tmp.path().join("2024/b.png"), 999);

    // add
    write_jpg(&tmp.path().join("2025/new.jpg"), 7);

    // delete
    fs::remove_file(tmp.path().join("2024/summer/c.jpg")).unwrap();

    let report = scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.updated, 2, "touch + modify: {report:?}");
    assert_eq!(report.added, 1);
    assert_eq!(report.removed, 1);
    assert_eq!(report.skipped, 2, "d.heic and e.mp4 unchanged");
    assert_eq!(count_files(&cat), 5);

    assert!(
        cat.conn()
            .query_row(
                "SELECT 1 FROM files WHERE path = ?1",
                [tmp.path().join("2024").join("summer").join("c.jpg").to_string_lossy().as_ref()],
                |r| r.get::<_, i64>(0)
            )
            .is_err(),
        "deleted file must be gone from the catalog"
    );
    let (_size, mtime, _media) = file_row(&cat, &touched);
    assert!(
        (mtime - new_mtime.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs_f64()).abs()
            < 0.001
    );
}

#[test]
fn deleted_subdirectory_removes_its_files() {
    let tmp = tempfile::tempdir().unwrap();
    make_library(tmp.path());

    let mut cat = open_catalog(tmp.path());
    scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();

    fs::remove_dir_all(tmp.path().join("2024/summer")).unwrap();

    let report = scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.removed, 1, "c.jpg is gone: {report:?}");
    assert_eq!(count_files(&cat), 4);
    assert_eq!(count_folders(&cat), 3, "empty folder row pruned");
}

#[test]
fn cancelled_scan_processes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    make_library(tmp.path());

    let mut cat = open_catalog(tmp.path());
    let cancel = AtomicBool::new(true);
    let report = scan_folder(&mut cat, tmp.path(), &cancel).unwrap();
    assert_eq!(report.added, 0);
    assert_eq!(count_files(&cat), 0);
}

#[test]
fn one_thousand_file_fixture_scans_and_rescans() {
    // Stand-in for the 1k fixture (deterministic layout, generated in-place).
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..1000 {
        let dir = tmp.path().join(format!("day{}", i % 50));
        fs::create_dir_all(&dir).unwrap();
        write_jpg(&dir.join(format!("img_{i:04}.jpg")), i);
    }

    let mut cat = open_catalog(tmp.path());
    let report = scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.added, 1000, "1k fixture populates N rows");
    assert_eq!(count_files(&cat), 1000);

    let report = scan_folder(&mut cat, tmp.path(), &AtomicBool::new(false)).unwrap();
    assert_eq!(report.skipped, 1000, "rescan re-reads nothing");
    assert_eq!(report.added + report.updated + report.removed, 0);
}
