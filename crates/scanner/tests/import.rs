//! TASK-01-017 integration: import from folder / removable drive.
//!
//! Import copies media files into the catalog location, names destination
//! folders by date (file date until the EXIF layer lands) or a custom
//! pattern, and — with "Exclude duplicates" on (the default, PLAN §2) —
//! skips files whose content hash is already cataloged, counting them in
//! the report. Cancellation must never leave a half-copied file: copies go
//! through a temp file + rename, and a cancelled copy cleans its temp.

use std::fs;
use std::io::{Read, Result as IoResult};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use fotobase_catalog::Catalog;
use fotobase_scanner::{content_hash, copy_file, import_folder, ImportOptions, Naming};

fn write_jpg(path: &Path, byte: u8, len: usize) {
    fs::write(path, vec![byte; len]).unwrap();
}

fn open_catalog(dir: &Path) -> Catalog {
    Catalog::open(dir.join("catalog.db")).unwrap()
}

/// Seed one cataloged file with the given content hash (as the lazy hashing
/// pass would have stored it).
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
    let mut out: Vec<String> = walk(dest, dest);
    out.sort();
    out
}

fn walk(root: &Path, dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(walk(root, &p));
        } else {
            out.push(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
    out
}

fn catalog_paths(cat: &Catalog) -> Vec<String> {
    let mut stmt = cat.conn().prepare("SELECT path FROM files ORDER BY path").unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().flatten().collect()
}

fn set_mtime(path: &Path, secs: u64) {
    filetime::set_file_mtime(path, filetime::FileTime::from_unix_time(secs as i64, 0)).unwrap();
}

// 2023-01-15 12:00:00 UTC
const MTIME: u64 = 1_673_784_000;

#[test]
fn duplicate_skipped_and_counted_when_exclude_is_on() {
    let src_dir = tempfile::tempdir().unwrap();
    let dst_dir = tempfile::tempdir().unwrap();
    let db_dir = tempfile::tempdir().unwrap();
    write_jpg(&src_dir.path().join("a.jpg"), 0xaa, 100);

    let mut cat = open_catalog(db_dir.path());
    let hash = content_hash(&src_dir.path().join("a.jpg")).unwrap();
    seed_hashed_file(&mut cat, "/lib/already.jpg", &hash);

    let opts = ImportOptions { exclude_duplicates: true, naming: Naming::Custom("in".into()) };
    let report = import_folder(
        &mut cat,
        src_dir.path(),
        dst_dir.path(),
        &opts,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();

    assert_eq!(report.copied, 0);
    assert_eq!(report.duplicates, 1, "the duplicate is counted in the report");
    assert_eq!(dest_files(dst_dir.path()).len(), 0, "duplicate is not copied");
    assert_eq!(catalog_paths(&cat).len(), 1, "catalog unchanged");
}

#[test]
fn duplicate_copied_when_exclude_is_off() {
    let src_dir = tempfile::tempdir().unwrap();
    let dst_dir = tempfile::tempdir().unwrap();
    let db_dir = tempfile::tempdir().unwrap();
    write_jpg(&src_dir.path().join("a.jpg"), 0xaa, 100);

    let mut cat = open_catalog(db_dir.path());
    let hash = content_hash(&src_dir.path().join("a.jpg")).unwrap();
    seed_hashed_file(&mut cat, "/lib/already.jpg", &hash);

    let opts = ImportOptions { exclude_duplicates: false, naming: Naming::Custom("in".into()) };
    let report = import_folder(
        &mut cat,
        src_dir.path(),
        dst_dir.path(),
        &opts,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();

    assert_eq!(report.copied, 1);
    assert_eq!(report.duplicates, 0);
    let copied = dst_dir.path().join("in").join("a.jpg");
    assert!(copied.exists(), "exclusion off → the file is copied");
    assert_eq!(fs::read(&copied).unwrap(), vec![0xaa; 100], "copy is byte-exact");

    // The imported row carries the content hash, so a later import (or the
    // thumb:// scheme) can resolve it.
    let paths = catalog_paths(&cat);
    assert_eq!(paths.len(), 2);
    let stored: String = cat
        .conn()
        .query_row(
            "SELECT hash FROM files WHERE path = ?1",
            [copied.to_string_lossy().as_ref()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, hash);
}

#[test]
fn date_naming_uses_the_file_date() {
    let src_dir = tempfile::tempdir().unwrap();
    let dst_dir = tempfile::tempdir().unwrap();
    let db_dir = tempfile::tempdir().unwrap();
    let src = src_dir.path().join("a.jpg");
    write_jpg(&src, 0x01, 10);
    set_mtime(&src, MTIME);

    let mut cat = open_catalog(db_dir.path());
    let opts = ImportOptions { exclude_duplicates: true, naming: Naming::DateFolders };
    import_folder(
        &mut cat,
        src_dir.path(),
        dst_dir.path(),
        &opts,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();

    assert_eq!(dest_files(dst_dir.path()), vec!["2023-01-15/a.jpg".to_string()]);
}

#[test]
fn custom_naming_places_files_under_the_pattern() {
    let src_dir = tempfile::tempdir().unwrap();
    let dst_dir = tempfile::tempdir().unwrap();
    let db_dir = tempfile::tempdir().unwrap();
    write_jpg(&src_dir.path().join("a.jpg"), 0x02, 10);

    let mut cat = open_catalog(db_dir.path());
    let opts = ImportOptions { exclude_duplicates: true, naming: Naming::Custom("trip".into()) };
    import_folder(
        &mut cat,
        src_dir.path(),
        dst_dir.path(),
        &opts,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();

    assert_eq!(dest_files(dst_dir.path()), vec!["trip/a.jpg".to_string()]);
}

#[test]
fn non_media_files_are_ignored() {
    let src_dir = tempfile::tempdir().unwrap();
    let dst_dir = tempfile::tempdir().unwrap();
    let db_dir = tempfile::tempdir().unwrap();
    fs::write(src_dir.path().join("notes.txt"), "not a photo").unwrap();

    let mut cat = open_catalog(db_dir.path());
    let opts = ImportOptions { exclude_duplicates: true, naming: Naming::Custom("in".into()) };
    let report = import_folder(
        &mut cat,
        src_dir.path(),
        dst_dir.path(),
        &opts,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();

    assert_eq!(report.copied, 0);
    assert_eq!(dest_files(dst_dir.path()).len(), 0);
}

#[test]
fn cancel_mid_import_copies_nothing_more_and_leaves_no_temp_files() {
    let src_dir = tempfile::tempdir().unwrap();
    let dst_dir = tempfile::tempdir().unwrap();
    let db_dir = tempfile::tempdir().unwrap();
    write_jpg(&src_dir.path().join("first.jpg"), 0x11, 10);
    write_jpg(&src_dir.path().join("second.jpg"), 0x22, 10);

    let mut cat = open_catalog(db_dir.path());
    let cancel = AtomicBool::new(false);
    let opts = ImportOptions { exclude_duplicates: true, naming: Naming::Custom("in".into()) };
    let report = import_folder(&mut cat, src_dir.path(), dst_dir.path(), &opts, &cancel, &|p| {
        // Cancel as soon as the first file is done.
        if p.files_done == 1 {
            cancel.store(true, Ordering::Relaxed);
        }
    })
    .unwrap();

    assert!(report.cancelled);
    assert_eq!(report.copied, 1);
    let done = dest_files(dst_dir.path());
    // Directory order decides which file is first — only that one survives,
    // fully copied; the rest of the batch never starts.
    assert_eq!(done.len(), 1, "only fully copied files survive");
    assert!(
        done.iter().all(|f| !f.ends_with(".part")),
        "no half-copied temp files at the destination"
    );
}

/// Reader that cancels the import after its first chunk — simulates a
/// mid-file cancellation deterministically, no timing involved.
struct CancelMidCopy<'a> {
    inner: fs::File,
    cancel: &'a AtomicBool,
    armed: bool,
}

impl Read for CancelMidCopy<'_> {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        if self.armed {
            self.cancel.store(true, Ordering::Relaxed);
            self.armed = false;
        }
        self.inner.read(buf)
    }
}

#[test]
fn cancel_mid_copy_leaves_no_partial_destination_file() {
    let src_dir = tempfile::tempdir().unwrap();
    let dst_dir = tempfile::tempdir().unwrap();
    let src = src_dir.path().join("big.jpg");
    write_jpg(&src, 0x33, 1 << 20); // 1 MiB, many chunks

    let cancel = AtomicBool::new(false);
    let dst = dst_dir.path().join("big.jpg");
    let file = fs::File::open(&src).unwrap();
    let mut reader = CancelMidCopy { inner: file, cancel: &cancel, armed: true };
    let copied = copy_file(&mut reader, dst_dir.path(), "big.jpg", &cancel).unwrap();
    assert!(!copied, "cancelled copy reports not-copied");
    assert!(!dst.exists(), "no partial file at the destination");
    assert!(!dst_dir.path().join("big.jpg.part").exists(), "the temp file is cleaned up");
}

#[test]
fn a_dest_name_collision_never_overwrites() {
    let src_dir = tempfile::tempdir().unwrap();
    let dst_dir = tempfile::tempdir().unwrap();
    let db_dir = tempfile::tempdir().unwrap();
    write_jpg(&src_dir.path().join("a.jpg"), 0x44, 10);
    // Something different already lives at the destination under the same name.
    fs::create_dir_all(dst_dir.path().join("in")).unwrap();
    write_jpg(&dst_dir.path().join("in").join("a.jpg"), 0x55, 10);

    let mut cat = open_catalog(db_dir.path());
    let opts = ImportOptions { exclude_duplicates: false, naming: Naming::Custom("in".into()) };
    let report = import_folder(
        &mut cat,
        src_dir.path(),
        dst_dir.path(),
        &opts,
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();

    assert_eq!(report.copied, 1);
    assert_eq!(
        fs::read(dst_dir.path().join("in").join("a.jpg")).unwrap(),
        vec![0x55; 10],
        "the existing file is never overwritten"
    );
    assert_eq!(fs::read(dst_dir.path().join("in").join("a-1.jpg")).unwrap(), vec![0x44; 10]);
}

#[test]
fn progress_reports_totals_and_cumulative_counts() {
    let src_dir = tempfile::tempdir().unwrap();
    let dst_dir = tempfile::tempdir().unwrap();
    let db_dir = tempfile::tempdir().unwrap();
    write_jpg(&src_dir.path().join("a.jpg"), 0x01, 10);
    write_jpg(&src_dir.path().join("b.jpg"), 0x02, 10);

    let mut cat = open_catalog(db_dir.path());
    let opts = ImportOptions { exclude_duplicates: true, naming: Naming::Custom("in".into()) };
    let seen = std::cell::RefCell::new(Vec::<(usize, usize)>::new());
    import_folder(&mut cat, src_dir.path(), dst_dir.path(), &opts, &AtomicBool::new(false), &|p| {
        seen.borrow_mut().push((p.files_done, p.files_total));
    })
    .unwrap();

    let seen = seen.into_inner();
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "files_done is cumulative");
    assert_eq!(seen.last().map(|(_, total)| *total), Some(2), "total is known up front");
}

/// An already-canceled import is a no-op, not a partial run.
#[test]
fn cancelled_up_front_imports_nothing() {
    let src_dir = tempfile::tempdir().unwrap();
    let dst_dir = tempfile::tempdir().unwrap();
    let db_dir = tempfile::tempdir().unwrap();
    write_jpg(&src_dir.path().join("a.jpg"), 0x01, 10);

    let mut cat = open_catalog(db_dir.path());
    let cancel = AtomicBool::new(true);
    let opts = ImportOptions { exclude_duplicates: true, naming: Naming::Custom("in".into()) };
    let report =
        import_folder(&mut cat, src_dir.path(), dst_dir.path(), &opts, &cancel, &|_| {}).unwrap();

    assert!(report.cancelled);
    assert_eq!(report.copied, 0);
    assert_eq!(dest_files(dst_dir.path()).len(), 0);
}

/// `content_hash` is the render cache-key convention: first 16 bytes of the
/// SHA-256, hex — so an imported file's thumb:// key matches its hash.
#[test]
fn content_hash_matches_the_thumb_cache_key_convention() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("x.jpg");
    fs::write(&p, b"hello world").unwrap();
    let h = content_hash(&p).unwrap();
    assert_eq!(h.len(), 32);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    let h2 = content_hash(&p).unwrap();
    assert_eq!(h, h2, "hashing is deterministic");
}
