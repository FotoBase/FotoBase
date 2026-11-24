//! Read-only-safe catalog source tests (TASK-09-002, TDD).
//!
//! A mobile catalog source is a NAS path or a Syncthing/iCloud-synced
//! folder that other processes write to while the app scans it. The scan
//! must never index a half-written file, never corrupt the catalog, and
//! never write to the source (PLAN §6 Phase 5, §3 rule 1).

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fotobase_catalog::Catalog;
use fotobase_scanner::{
    scan_folder_with, scan_network_folder, CatalogSource, EntryMeta, FsOps, NetworkScanOptions,
    NetworkScanState, RealFs, ScanOptions, ScanProgress, SourceError, SourceKind, WriteIntent,
};

fn write_jpg(path: &Path, len: usize) {
    fs::write(path, vec![0xff; len]).unwrap();
}

fn catalog_at(dir: &Path) -> Catalog {
    Catalog::open(dir.join("catalog.db")).unwrap()
}

/// (path, size) of every catalogued file.
fn catalog_files(cat: &Catalog) -> BTreeMap<String, i64> {
    let mut stmt = cat.conn().prepare("SELECT path, size FROM files").unwrap();
    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .unwrap()
        .flatten()
        .collect()
}

/// (path, size) of every media file on disk under `root`.
fn disk_files(root: &Path) -> BTreeMap<String, i64> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "jpg") {
                out.insert(path.to_string_lossy().into_owned(), meta.len() as i64);
            }
        }
    }
    out
}

fn integrity_ok(cat: &Catalog) -> bool {
    let res: String = cat.conn().query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
    res == "ok"
}

/// Run a scan that mutates the tree once the walk has finished but before
/// anything is indexed: the final progress callback carries the complete
/// walk totals, so a mutation there lands exactly in the race window.
fn scan_with_mutation(
    cat: &mut Catalog,
    root: &Path,
    total_files: usize,
    mutate: impl Fn(),
) -> fotobase_scanner::ScanReport {
    let done = Cell::new(false);
    scan_folder_with(cat, root, &AtomicBool::new(false), &ScanOptions::default(), &|p| {
        if p.files_seen == total_files && !done.get() {
            done.set(true);
            mutate();
        }
    })
    .unwrap()
}

/// Unit criterion: a file that changes size mid-scan is skipped and
/// re-queued — never indexed at its partial size.
#[test]
fn file_changed_mid_scan_is_skipped_and_requeued_not_partially_indexed() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).unwrap();
    write_jpg(&lib.join("a.jpg"), 10);
    write_jpg(&lib.join("b.jpg"), 20);
    let mut cat = catalog_at(tmp.path());

    let b = lib.join("b.jpg");
    let report = scan_with_mutation(&mut cat, &lib, 2, || write_jpg(&b, 999));

    assert_eq!(report.added, 1, "only the stable file is indexed");
    assert_eq!(report.requeued, vec![b.to_string_lossy().into_owned()]);
    assert!(report.needs_rescan());
    let files = catalog_files(&cat);
    assert_eq!(files.len(), 1);
    assert!(!files.contains_key(b.to_string_lossy().as_ref()));

    // The next pass sees the settled file and indexes it at its final size.
    let report = scan_folder_with(
        &mut cat,
        &lib,
        &AtomicBool::new(false),
        &ScanOptions::default(),
        &|_: ScanProgress| {},
    )
    .unwrap();
    assert_eq!(report.added, 1);
    assert!(report.requeued.is_empty());
    assert_eq!(catalog_files(&cat).get(b.to_string_lossy().as_ref()), Some(&999));
}

/// An already-indexed file that changes mid-scan keeps its last consistent
/// row until the next pass; it is not half-updated.
#[test]
fn indexed_file_changed_mid_scan_keeps_previous_row() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).unwrap();
    write_jpg(&lib.join("a.jpg"), 10);
    let mut cat = catalog_at(tmp.path());
    scan_with_mutation(&mut cat, &lib, 1, || {});

    let a = lib.join("a.jpg");
    write_jpg(&a, 50); // changed since the last scan…
    let report = scan_with_mutation(&mut cat, &lib, 1, || write_jpg(&a, 75)); // …and again mid-scan

    assert_eq!(report.updated, 0);
    assert_eq!(report.requeued.len(), 1);
    assert_eq!(catalog_files(&cat).get(a.to_string_lossy().as_ref()), Some(&10));
}

/// A file deleted between the walk and the index is requeued, not a crash
/// and not a phantom row.
#[test]
fn file_vanishing_mid_scan_is_not_indexed() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).unwrap();
    write_jpg(&lib.join("a.jpg"), 10);
    write_jpg(&lib.join("b.jpg"), 20);
    let mut cat = catalog_at(tmp.path());

    let b = lib.join("b.jpg");
    let report = scan_with_mutation(&mut cat, &lib, 2, || fs::remove_file(&b).unwrap());

    assert_eq!(report.added, 1);
    assert_eq!(report.requeued.len(), 1);
    assert!(integrity_ok(&cat));
}

/// Integration criterion: scan while a background thread mutates the tree
/// (create, grow, delete). The catalog ends consistent with the settled
/// disk state and never crashes.
#[test]
fn scan_while_background_process_mutates_ends_consistent() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir_all(lib.join("sub")).unwrap();
    for i in 0..20 {
        write_jpg(&lib.join(format!("img{i:02}.jpg")), 100 + i);
    }
    let mut cat = catalog_at(tmp.path());
    let source = CatalogSource::open(&lib).unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let mutator = {
        let lib = lib.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut n = 0usize;
            while !stop.load(Ordering::Relaxed) {
                write_jpg(&lib.join("sub").join(format!("new{:03}.jpg", n % 15)), 10 + n);
                write_jpg(&lib.join(format!("img{:02}.jpg", n % 20)), 500 + n);
                let _ =
                    fs::remove_file(lib.join("sub").join(format!("new{:03}.jpg", (n + 7) % 15)));
                n += 1;
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    };

    for _ in 0..15 {
        let _ = source.scan(&mut cat, &AtomicBool::new(false), &|_| {}).unwrap();
    }
    stop.store(true, Ordering::Relaxed);
    mutator.join().unwrap();

    // Mutator stopped: the very next settled scan must match the disk.
    let report = source.scan_settled(&mut cat, &AtomicBool::new(false), 3, &|_| {}).unwrap();
    assert!(!report.needs_rescan());
    assert!(integrity_ok(&cat));
    assert_eq!(catalog_files(&cat), disk_files(&lib));
}

/// Opening and scanning a source writes nothing to it: the tree (names,
/// sizes, mtimes) is identical before and after, and a read-only source
/// directory scans fine.
#[test]
fn open_and_scan_never_write_to_the_source() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir_all(lib.join("sub")).unwrap();
    write_jpg(&lib.join("a.jpg"), 10);
    write_jpg(&lib.join("sub/b.jpg"), 20);
    fs::write(lib.join("sub/b.jpg.xmp"), "<x/>").unwrap();

    let snapshot = |root: &Path| -> BTreeMap<String, (u64, std::time::SystemTime)> {
        let mut out = BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap().flatten() {
                let meta = entry.metadata().unwrap();
                out.insert(
                    entry.path().to_string_lossy().into_owned(),
                    (meta.len(), meta.modified().unwrap()),
                );
                if meta.is_dir() {
                    stack.push(entry.path());
                }
            }
        }
        out
    };
    let before = snapshot(&lib);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for dir in [lib.clone(), lib.join("sub")] {
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
        }
    }

    let mut cat = catalog_at(tmp.path());
    let source = CatalogSource::open(&lib).unwrap();
    let report = source.scan_settled(&mut cat, &AtomicBool::new(false), 3, &|_| {}).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for dir in [lib.clone(), lib.join("sub")] {
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    assert_eq!(report.added, 2);
    assert_eq!(snapshot(&lib), before, "source tree must be untouched");
}

#[test]
fn background_writes_are_refused_and_user_actions_allowed() {
    let tmp = tempfile::tempdir().unwrap();
    let source = CatalogSource::open(tmp.path()).unwrap();
    let inside = tmp.path().join("a.jpg.xmp");

    assert!(matches!(
        source.authorize_write(&inside, WriteIntent::Background),
        Err(SourceError::WriteNotAuthorized { .. })
    ));
    assert!(source.authorize_write(&inside, WriteIntent::UserAction).is_ok());

    // Paths outside the source (e.g. the catalog cache) are not guarded.
    let outside = tempfile::tempdir().unwrap();
    assert!(source
        .authorize_write(&outside.path().join("catalog.db"), WriteIntent::Background)
        .is_ok());
}

#[test]
fn open_rejects_missing_or_non_directory_source() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(matches!(
        CatalogSource::open(&tmp.path().join("nope")),
        Err(SourceError::Unreachable { .. })
    ));
    let file = tmp.path().join("f.jpg");
    write_jpg(&file, 1);
    assert!(matches!(CatalogSource::open(&file), Err(SourceError::NotADirectory { .. })));
}

/// Filesystem whose `stat` reports a grown file for one path, as if a sync
/// client was still writing it after the directory listing.
struct GrowingFs {
    inner: RealFs,
    grow: PathBuf,
}

impl FsOps for GrowingFs {
    fn read_dir(&self, dir: &Path) -> io::Result<Vec<EntryMeta>> {
        self.inner.read_dir(dir)
    }
    fn probe(&self, root: &Path) -> bool {
        self.inner.probe(root)
    }
    fn stat(&self, path: &Path) -> io::Result<Option<EntryMeta>> {
        let Some(mut meta) = self.inner.stat(path)? else { return Ok(None) };
        if path == self.grow {
            meta.size += 1;
        }
        Ok(Some(meta))
    }
}

/// Network (NAS) scans apply the same stability rule.
#[test]
fn network_scan_requeues_files_that_change_after_listing() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).unwrap();
    write_jpg(&lib.join("a.jpg"), 10);
    write_jpg(&lib.join("b.jpg"), 20);
    let mut cat = catalog_at(tmp.path());

    let options = NetworkScanOptions {
        fs: Arc::new(GrowingFs { inner: RealFs, grow: lib.join("b.jpg") }),
        ops_per_sec: 0,
        ..NetworkScanOptions::default()
    };
    let report =
        scan_network_folder(&mut cat, &lib, &AtomicBool::new(false), &options, &|_| {}).unwrap();

    assert_eq!(report.state, NetworkScanState::Completed);
    assert_eq!(report.added, 1);
    assert_eq!(report.requeued, vec![lib.join("b.jpg").to_string_lossy().into_owned()]);
    assert_eq!(catalog_files(&cat).len(), 1);
}

/// A source forced to the network backend scans through the throttled path
/// and reports the same unified report.
#[test]
fn network_kind_source_scans_and_unifies_report() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).unwrap();
    write_jpg(&lib.join("a.jpg"), 10);
    let mut cat = catalog_at(tmp.path());

    let source = CatalogSource::open_as(&lib, SourceKind::Network)
        .unwrap()
        .with_network_options(NetworkScanOptions { ops_per_sec: 0, ..Default::default() });
    assert_eq!(source.kind(), SourceKind::Network);
    let report = source.scan(&mut cat, &AtomicBool::new(false), &|_| {}).unwrap();
    assert_eq!(report.added, 1);
}

/// A disconnected network source surfaces a typed error, deletes nothing.
#[test]
fn disconnected_network_source_reports_unreachable_and_keeps_catalog() {
    struct DownFs;
    impl FsOps for DownFs {
        fn read_dir(&self, _: &Path) -> io::Result<Vec<EntryMeta>> {
            Err(io::Error::new(io::ErrorKind::TimedOut, "share unreachable"))
        }
        fn probe(&self, _: &Path) -> bool {
            false
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).unwrap();
    write_jpg(&lib.join("a.jpg"), 10);
    let mut cat = catalog_at(tmp.path());
    let up = CatalogSource::open(&lib).unwrap();
    up.scan(&mut cat, &AtomicBool::new(false), &|_| {}).unwrap();

    let down = CatalogSource::open_as(&lib, SourceKind::Network).unwrap().with_network_options(
        NetworkScanOptions {
            fs: Arc::new(DownFs),
            ops_per_sec: 0,
            probe_interval: Duration::from_millis(1),
            ..Default::default()
        },
    );
    let err = down.scan(&mut cat, &AtomicBool::new(false), &|_| {}).unwrap_err();
    assert!(matches!(err, SourceError::Unreachable { .. }));
    assert_eq!(catalog_files(&cat).len(), 1, "offline browsing keeps the cached catalog");
}
