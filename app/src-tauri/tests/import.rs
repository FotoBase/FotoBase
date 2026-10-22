//! TASK-01-017 integration: the Tauri shell's import wrapper.
//!
//! `run_import` is the plain-Rust half of the `start_import` command: it
//! owns a catalog connection, runs the scanner's import engine, and streams
//! progress to the UI as plain counters (no image bytes over IPC). Cancel
//! leaves the destination without half-copied files (the engine guarantees
//! that per file).

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use fotobase_catalog::Catalog;
use fotobase_lib::import::{run_import, ImportProgress, ImportReport};
use fotobase_scanner::{ImportOptions, Naming};

fn write_jpg(path: &Path, byte: u8, len: usize) {
    fs::write(path, vec![byte; len]).unwrap();
}

fn seed_hashed_file(cat: &mut Catalog, path: &str, hash: &str) {
    cat.seed_folder("/lib").unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, hash, media_type)
             VALUES (1, ?1, 'x.jpg', 1, 0.0, ?2, 'image')",
            rusqlite::params![path, hash],
        )
        .unwrap();
}

fn dest_files(dest: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    let mut out = Vec::new();
    walk(dest, dest, &mut out);
    out.sort();
    out
}

#[test]
fn run_import_skips_cataloged_duplicates_and_reports_progress() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let db = tempfile::tempdir().unwrap();
    write_jpg(&src.path().join("a.jpg"), 0xaa, 100);

    let cat_path = db.path().join("catalog.db");
    {
        let mut cat = Catalog::open(&cat_path).unwrap();
        // Same convention the scanner/render hashing pass stores.
        let hash = fotobase_scanner::content_hash(&src.path().join("a.jpg")).unwrap();
        seed_hashed_file(&mut cat, "/lib/already.jpg", &hash);
    }

    let opts = ImportOptions { exclude_duplicates: true, naming: Naming::Custom("in".into()) };
    let events = std::cell::RefCell::new(Vec::<ImportProgress>::new());
    let report =
        run_import(&cat_path, src.path(), dst.path(), &opts, &AtomicBool::new(false), &|p| {
            events.borrow_mut().push(p)
        })
        .unwrap();

    assert_eq!(report.duplicates, 1, "the duplicate is skipped and counted");
    assert_eq!(report.copied, 0);
    assert!(dest_files(dst.path()).is_empty());
    assert!(!events.borrow().is_empty(), "progress reaches the UI");
}

#[test]
fn run_import_copies_new_files_and_reports_copied() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let db = tempfile::tempdir().unwrap();
    write_jpg(&src.path().join("a.jpg"), 0xab, 50);

    let cat_path = db.path().join("catalog.db");
    let opts = ImportOptions { exclude_duplicates: true, naming: Naming::DateFolders };
    let report =
        run_import(&cat_path, src.path(), dst.path(), &opts, &AtomicBool::new(false), &|_| {})
            .unwrap();

    assert_eq!(report.copied, 1);
    assert_eq!(dest_files(dst.path()).len(), 1, "file landed under a date folder");
    let cat = Catalog::open(&cat_path).unwrap();
    assert_eq!(cat.list_files(10).unwrap().len(), 1, "the copy is cataloged");
}

#[test]
fn run_import_reports_cancellation() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let db = tempfile::tempdir().unwrap();
    write_jpg(&src.path().join("a.jpg"), 0xac, 10);
    write_jpg(&src.path().join("b.jpg"), 0xad, 10);

    let cancel = AtomicBool::new(false);
    let opts = ImportOptions { exclude_duplicates: true, naming: Naming::Custom("in".into()) };
    let report: ImportReport = run_import(
        db.path().join("catalog.db").as_path(),
        src.path(),
        dst.path(),
        &opts,
        &cancel,
        &|p| {
            if p.files_done == 1 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
    )
    .unwrap();

    assert!(report.cancelled);
    assert_eq!(report.copied, 1);
    // Directory order decides which file is first — the point is that the
    // batch stops after one fully copied file.
    let done = dest_files(dst.path());
    assert_eq!(done.len(), 1);
    assert!(done[0].starts_with("in/") && done[0].ends_with(".jpg"));
}
