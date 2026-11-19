//! TASK-08-014: Backup Manager incremental backup sets (PLAN §2L "Backup
//! Pictures", §6 P4 "Incremental backup + restore", §5 incremental-rebuild
//! skip). A set is a self-describing manifest plus content-addressed blobs;
//! the second set holds only the delta and references the first.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fotobase_catalog::backup::{
    library_sources, list_sets, read_manifest, run_backup, BackupError, BackupHooks,
    BackupProgress, BackupSource, EntryKind, Manifest, BACKUP_DIR,
};
use fotobase_catalog::{sidecar_path, Catalog};

struct Lib {
    dir: tempfile::TempDir,
    cat: Catalog,
    settings: PathBuf,
}

impl Lib {
    fn new() -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("catalog.db")).unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(&settings, br#"{"language":"en"}"#).unwrap();
        let mut lib = Lib { dir, cat, settings };
        for name in ["a.jpg", "b.jpg", "c.jpg"] {
            lib.photo(name);
        }
        lib
    }

    fn photos(&self) -> PathBuf {
        self.dir.path().join("photos")
    }

    fn photo(&mut self, name: &str) {
        let photos = self.photos();
        std::fs::create_dir_all(&photos).unwrap();
        let original = photos.join(name);
        std::fs::write(&original, format!("pixels of {name}")).unwrap();
        std::fs::write(sidecar_path(original.to_str().unwrap()), xmp(name, 1)).unwrap();
        let folder_id = self.cat.seed_folder(photos.to_str().unwrap()).unwrap();
        self.cat
            .conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
                 VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
                rusqlite::params![folder_id, original.to_str().unwrap(), name],
            )
            .unwrap();
    }

    fn sidecar(&self, name: &str) -> PathBuf {
        sidecar_path(self.photos().join(name).to_str().unwrap())
    }

    fn sources(&self) -> Vec<BackupSource> {
        let mut sources = library_sources(&self.cat).unwrap();
        sources.push(BackupSource { kind: EntryKind::Settings, path: self.settings.clone() });
        sources
    }
}

fn xmp(name: &str, rating: u8) -> String {
    format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
         xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
         <rdf:Description rdf:about=\"{name}\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" \
         xmp:Rating=\"{rating}\"/></rdf:RDF></x:xmpmeta>"
    )
}

fn root(dest: &Path) -> PathBuf {
    dest.join(BACKUP_DIR)
}

/// Blob names stored inside one set directory.
fn blobs(dest: &Path, set: &str) -> BTreeSet<String> {
    std::fs::read_dir(root(dest).join(set).join("files"))
        .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

fn manifest(dest: &Path, set: &str) -> Manifest {
    read_manifest(&root(dest).join(set)).unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Every entry resolves to a blob whose bytes hash to the recorded digest —
/// a set restores from its manifest alone, no app state needed.
fn assert_self_describing(dest: &Path, m: &Manifest) {
    for e in &m.entries {
        let blob = root(dest).join(&e.set).join("files").join(&e.sha256);
        let bytes = std::fs::read(&blob).unwrap_or_else(|err| panic!("{}: {err}", blob.display()));
        assert_eq!(sha256_hex(&bytes), e.sha256, "{}", e.path);
        assert_eq!(bytes.len() as u64, e.size, "{}", e.path);
    }
}

/// The `.partial` staging dir of the run in progress.
fn staging(dest: &Path) -> PathBuf {
    std::fs::read_dir(root(dest))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "partial"))
        .expect("staging dir exists mid-run")
}

/// Any `manifest.json` anywhere under the backup root.
fn manifests_on_disk(dest: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.file_name().is_some_and(|n| n == "manifest.json") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(&root(dest), &mut out);
    out
}

#[test]
fn first_set_is_full_and_self_describing() {
    let lib = Lib::new();
    let dest = tempfile::tempdir().unwrap();

    let report =
        run_backup(dest.path(), Some(&lib.cat), &lib.sources(), &BackupHooks::none()).unwrap();

    let m = manifest(dest.path(), &report.set_id);
    assert_eq!(m.parent, None, "first set is a full backup");
    let kinds: Vec<EntryKind> = m.entries.iter().map(|e| e.kind).collect();
    assert_eq!(kinds.iter().filter(|k| **k == EntryKind::Catalog).count(), 1);
    assert_eq!(kinds.iter().filter(|k| **k == EntryKind::Sidecar).count(), 3);
    assert_eq!(kinds.iter().filter(|k| **k == EntryKind::Settings).count(), 1);
    assert!(m.entries.iter().all(|e| e.set == report.set_id));
    assert_eq!(report.files_copied, m.entries.len());
    assert_self_describing(dest.path(), &m);

    // The catalog blob is a consistent SQLite snapshot of the live catalog.
    let cat_entry = m.entries.iter().find(|e| e.kind == EntryKind::Catalog).unwrap();
    assert_eq!(Path::new(&cat_entry.path), lib.cat.path());
    let snap = root(dest.path()).join(&cat_entry.set).join("files").join(&cat_entry.sha256);
    let restored = rusqlite::Connection::open(&snap).unwrap();
    let n: i64 = restored.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 3);
}

#[test]
fn second_set_holds_only_the_delta_and_references_the_first() {
    let lib = Lib::new();
    let dest = tempfile::tempdir().unwrap();
    let first =
        run_backup(dest.path(), Some(&lib.cat), &lib.sources(), &BackupHooks::none()).unwrap();

    // One sidecar changes (new content, newer mtime).
    let changed = lib.sidecar("b.jpg");
    std::fs::write(&changed, xmp("b.jpg", 5)).unwrap();
    filetime::set_file_mtime(&changed, filetime::FileTime::from_unix_time(2_000_000_000, 0))
        .unwrap();

    let second =
        run_backup(dest.path(), Some(&lib.cat), &lib.sources(), &BackupHooks::none()).unwrap();

    let m1 = manifest(dest.path(), &first.set_id);
    let m2 = manifest(dest.path(), &second.set_id);
    assert_eq!(m2.parent.as_deref(), Some(first.set_id.as_str()));
    assert_eq!(m2.entries.len(), m1.entries.len(), "every file is still described");
    assert_self_describing(dest.path(), &m2);

    let changed_entry =
        m2.entries.iter().find(|e| Path::new(&e.path) == changed).expect("changed sidecar listed");
    assert_eq!(changed_entry.set, second.set_id, "the delta lives in the new set");
    assert_eq!(changed_entry.sha256, sha256_hex(xmp("b.jpg", 5).as_bytes()));

    // Unchanged sidecars and settings reference set 1; only the catalog
    // snapshot may also be new (its bytes change with the live database).
    for e in m2.entries.iter().filter(|e| e.kind != EntryKind::Catalog) {
        if Path::new(&e.path) != changed {
            assert_eq!(e.set, first.set_id, "{} is unchanged", e.path);
        }
    }
    let catalog_sha = &m2.entries.iter().find(|e| e.kind == EntryKind::Catalog).unwrap().sha256;
    let mut delta = blobs(dest.path(), &second.set_id);
    delta.remove(catalog_sha);
    assert_eq!(delta, BTreeSet::from([changed_entry.sha256.clone()]));
    assert!(second.files_reused >= 4, "{second:?}");
}

#[test]
fn unplugged_destination_mid_run_leaves_no_manifest() {
    let lib = Lib::new();
    let dest = tempfile::tempdir().unwrap();
    let dest_path = dest.path().to_path_buf();
    let unplugged = std::cell::Cell::new(false);
    let progress = |_: &BackupProgress| {
        if !unplugged.get() {
            unplugged.set(true);
            // The drive vanishes: everything the run staged is gone.
            std::fs::remove_dir_all(staging(&dest_path)).unwrap();
        }
    };
    let hooks = BackupHooks { progress: &progress, ..BackupHooks::none() };

    let err = run_backup(dest.path(), Some(&lib.cat), &lib.sources(), &hooks).unwrap_err();

    assert!(matches!(err, BackupError::Destination { .. }), "{err}");
    assert!(manifests_on_disk(dest.path()).is_empty());
    assert!(list_sets(dest.path()).unwrap().is_empty());
    // Replugged: the next run is a clean full backup, not a broken increment.
    let next =
        run_backup(dest.path(), Some(&lib.cat), &lib.sources(), &BackupHooks::none()).unwrap();
    assert_eq!(manifest(dest.path(), &next.set_id).parent, None);
}

#[cfg(unix)]
#[test]
fn destination_turning_read_only_mid_run_leaves_no_manifest() {
    use std::os::unix::fs::PermissionsExt;

    let lib = Lib::new();
    let dest = tempfile::tempdir().unwrap();
    let dest_path = dest.path().to_path_buf();
    let locked = std::cell::RefCell::new(Vec::<PathBuf>::new());
    let progress = |_: &BackupProgress| {
        if locked.borrow().is_empty() {
            let dirs = vec![staging(&dest_path).join("files"), staging(&dest_path)];
            for d in &dirs {
                std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o555)).unwrap();
            }
            *locked.borrow_mut() = dirs;
        }
    };
    let hooks = BackupHooks { progress: &progress, ..BackupHooks::none() };

    let result = run_backup(dest.path(), Some(&lib.cat), &lib.sources(), &hooks);
    for d in locked.borrow().iter().rev() {
        let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o755));
    }
    // Root ignores permission bits; there is nothing to simulate then.
    if is_root() {
        return;
    }

    assert!(matches!(result, Err(BackupError::Destination { .. })), "{result:?}");
    assert!(manifests_on_disk(dest.path()).is_empty());
    assert!(list_sets(dest.path()).unwrap().is_empty());
}

#[cfg(unix)]
fn is_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
}

#[test]
fn unwritable_destination_is_rejected_before_anything_runs() {
    let lib = Lib::new();
    let dest = tempfile::tempdir().unwrap();
    // A plain file where the backup folder would go: nothing can be created.
    let blocked = dest.path().join("not-a-dir");
    std::fs::write(&blocked, b"x").unwrap();

    let err =
        run_backup(&blocked, Some(&lib.cat), &lib.sources(), &BackupHooks::none()).unwrap_err();

    assert!(matches!(err, BackupError::Unavailable { .. }), "{err}");
}

#[test]
fn cancelled_run_records_no_set() {
    let lib = Lib::new();
    let dest = tempfile::tempdir().unwrap();
    let seen = std::cell::Cell::new(0usize);
    let progress = |_: &BackupProgress| seen.set(seen.get() + 1);
    let cancelled = || seen.get() >= 2;
    let hooks = BackupHooks { progress: &progress, cancelled: &cancelled, ..BackupHooks::none() };

    let err = run_backup(dest.path(), Some(&lib.cat), &lib.sources(), &hooks).unwrap_err();

    assert!(matches!(err, BackupError::Cancelled), "{err}");
    assert!(manifests_on_disk(dest.path()).is_empty());
    assert!(!root(dest.path()).read_dir().unwrap().flatten().any(|e| e.path().is_dir()));
}

#[test]
fn list_sets_summarizes_each_set_oldest_first() {
    let lib = Lib::new();
    let dest = tempfile::tempdir().unwrap();
    let first =
        run_backup(dest.path(), Some(&lib.cat), &lib.sources(), &BackupHooks::none()).unwrap();
    let second =
        run_backup(dest.path(), Some(&lib.cat), &lib.sources(), &BackupHooks::none()).unwrap();

    let sets = list_sets(dest.path()).unwrap();

    let ids: Vec<&str> = sets.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, [first.set_id.as_str(), second.set_id.as_str()]);
    assert_eq!(sets[0].files, 5);
    assert_eq!(sets[0].new_files, 5);
    assert_eq!(sets[1].parent.as_deref(), Some(first.set_id.as_str()));
    assert!(sets[1].new_files <= 1, "nothing but the catalog snapshot may change");
}
