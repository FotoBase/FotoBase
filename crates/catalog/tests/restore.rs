//! TASK-08-015: restore from backup sets (PLAN §6 P4 acceptance "Backup →
//! wipe → restore reproduces library and sidecars byte-identical", §9 Data
//! safety). A set is validated in full before anything is written, and the
//! swap into the library is all-or-nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fotobase_catalog::backup::{
    library_sources, read_manifest, run_backup, BackupHooks, BackupSource, EntryKind, BACKUP_DIR,
};
use fotobase_catalog::restore::{
    recover_interrupted, restore_set, validate_set, Fault, RestoreError, RestoreHooks,
    RestoreOptions,
};
use fotobase_catalog::{sidecar_path, Catalog};
use sha2::{Digest, Sha256};

struct Lib {
    dir: tempfile::TempDir,
    catalog_path: PathBuf,
    settings: PathBuf,
    sources: Vec<BackupSource>,
    set: String,
    dest: tempfile::TempDir,
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn xmp(name: &str, rating: u8) -> String {
    format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
         xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
         <rdf:Description rdf:about=\"{name}\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" \
         xmp:Rating=\"{rating}\"/><!-- unknown nodes survive --></rdf:RDF></x:xmpmeta>"
    )
}

impl Lib {
    /// Three photos with sidecars, settings, and two backup sets (the second
    /// changes one sidecar, so the restore walks a chain).
    fn backed_up() -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let catalog_path = dir.path().join("catalog.db");
        let mut cat = Catalog::open(&catalog_path).unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir_all(&photos).unwrap();
        let folder_id = cat.seed_folder(photos.to_str().unwrap()).unwrap();
        for name in ["a.jpg", "b.jpg", "c.jpg"] {
            let original = photos.join(name);
            std::fs::write(&original, format!("pixels of {name}")).unwrap();
            std::fs::write(sidecar_path(original.to_str().unwrap()), xmp(name, 1)).unwrap();
            cat.conn()
                .execute(
                    "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
                     VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
                    rusqlite::params![folder_id, original.to_str().unwrap(), name],
                )
                .unwrap();
        }
        let settings = dir.path().join("settings.json");
        std::fs::write(&settings, br#"{"language":"en"}"#).unwrap();
        let mut sources = library_sources(&cat).unwrap();
        sources.push(BackupSource { kind: EntryKind::Settings, path: settings.clone() });

        let dest = tempfile::tempdir().unwrap();
        run_backup(dest.path(), Some(&cat), &sources, &BackupHooks::none()).unwrap();
        let changed = sidecar_path(photos.join("b.jpg").to_str().unwrap());
        std::fs::write(&changed, xmp("b.jpg", 5)).unwrap();
        filetime::set_file_mtime(&changed, filetime::FileTime::from_unix_time(2_000_000_000, 0))
            .unwrap();
        let second = run_backup(dest.path(), Some(&cat), &sources, &BackupHooks::none()).unwrap();
        drop(cat);
        Lib { dir, catalog_path, settings, sources, set: second.set_id, dest }
    }

    fn photos(&self) -> PathBuf {
        self.dir.path().join("photos")
    }

    fn opts(&self, confirm: bool) -> RestoreOptions {
        RestoreOptions { catalog_path: self.catalog_path.clone(), confirm_overwrite: confirm }
    }

    /// Every sidecar, settings file and original, by path.
    fn user_files(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut out = BTreeMap::new();
        for src in self.sources.iter().filter(|s| s.kind != EntryKind::Catalog) {
            if let Ok(bytes) = std::fs::read(&src.path) {
                out.insert(src.path.clone(), bytes);
            }
        }
        for name in ["a.jpg", "b.jpg", "c.jpg"] {
            let p = self.photos().join(name);
            out.insert(p.clone(), std::fs::read(p).unwrap());
        }
        out
    }

    /// Delete the catalog (with a stale WAL left behind), sidecars, settings.
    fn wipe(&self) {
        let _ = std::fs::remove_file(&self.catalog_path);
        std::fs::write(self.catalog_path.with_extension("db-wal"), b"stale wal").unwrap();
        for src in self.sources.iter().filter(|s| s.kind != EntryKind::Catalog) {
            let _ = std::fs::remove_file(&src.path);
        }
    }

    fn set_dir(&self) -> PathBuf {
        self.dest.path().join(BACKUP_DIR).join(&self.set)
    }

    /// Blob in the newest set that holds a sidecar, for corruption tests.
    fn some_blob(&self) -> PathBuf {
        let m = read_manifest(&self.set_dir()).unwrap();
        let e = m.entries.iter().find(|e| e.kind == EntryKind::Sidecar && e.set != self.set);
        let e = e.expect("an entry served from the first set");
        self.dest.path().join(BACKUP_DIR).join(&e.set).join("files").join(&e.sha256)
    }
}

/// Files left behind by a restore's staging or rollback machinery.
fn leftovers(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.to_string_lossy().contains(".fb-restore") {
                out.push(p);
            }
        }
    }
    walk(dir, &mut out);
    out
}

#[test]
fn backup_wipe_restore_is_byte_identical() {
    let lib = Lib::backed_up();
    let before = lib.user_files();
    let originals: Vec<_> = ["a.jpg", "b.jpg", "c.jpg"]
        .iter()
        .map(|n| sha(&std::fs::read(lib.photos().join(n)).unwrap()))
        .collect();
    lib.wipe();

    let report =
        restore_set(lib.dest.path(), &lib.set, &lib.opts(false), &RestoreHooks::none()).unwrap();

    assert_eq!(report.files_restored, 5);
    assert!(report.skipped.is_empty(), "{report:?}");
    assert_eq!(lib.user_files(), before, "sidecars, settings and originals are byte-identical");
    let after: Vec<_> = ["a.jpg", "b.jpg", "c.jpg"]
        .iter()
        .map(|n| sha(&std::fs::read(lib.photos().join(n)).unwrap()))
        .collect();
    assert_eq!(after, originals, "originals are untouched");

    // The catalog is the set's snapshot, byte for byte, and opens cleanly
    // (the stale WAL left by the wipe did not survive).
    let m = read_manifest(&lib.set_dir()).unwrap();
    let cat_entry = m.entries.iter().find(|e| e.kind == EntryKind::Catalog).unwrap();
    assert_eq!(sha(&std::fs::read(&lib.catalog_path).unwrap()), cat_entry.sha256);
    assert!(!lib.catalog_path.with_extension("db-wal").exists());
    let cat = Catalog::open(&lib.catalog_path).unwrap();
    let n: i64 = cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 3);
    assert!(leftovers(lib.dir.path()).is_empty());
}

#[test]
fn an_older_set_restores_its_own_state() {
    let lib = Lib::backed_up();
    let first = fotobase_catalog::backup::list_sets(lib.dest.path()).unwrap().remove(0).id;
    lib.wipe();

    restore_set(lib.dest.path(), &first, &lib.opts(false), &RestoreHooks::none()).unwrap();

    let b = std::fs::read_to_string(sidecar_path(lib.photos().join("b.jpg").to_str().unwrap()))
        .unwrap();
    assert_eq!(b, xmp("b.jpg", 1), "set 1 predates the rating change");
}

#[test]
fn a_corrupt_blob_is_rejected_before_anything_is_written() {
    let lib = Lib::backed_up();
    lib.wipe();
    std::fs::write(lib.some_blob(), b"bit rot").unwrap();

    let err =
        restore_set(lib.dest.path(), &lib.set, &lib.opts(true), &RestoreHooks::none()).unwrap_err();

    let RestoreError::Corrupt { problems, .. } = &err else { panic!("{err}") };
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].path.ends_with(".xmp"), "names the affected file: {problems:?}");
    assert!(!lib.catalog_path.exists(), "no catalog written");
    assert!(!lib.settings.exists(), "no settings written");
    assert!(!sidecar_path(lib.photos().join("a.jpg").to_str().unwrap()).exists());
    assert!(leftovers(lib.dir.path()).is_empty());
    assert!(validate_set(lib.dest.path(), &lib.set).is_err());
}

#[test]
fn a_missing_blob_is_reported_with_its_file() {
    let lib = Lib::backed_up();
    std::fs::remove_file(lib.some_blob()).unwrap();

    let err = validate_set(lib.dest.path(), &lib.set).unwrap_err();

    let RestoreError::Corrupt { problems, .. } = &err else { panic!("{err}") };
    assert!(problems.iter().any(|p| p.reason.contains("missing")), "{problems:?}");
}

#[test]
fn unknown_set_is_not_found() {
    let lib = Lib::backed_up();
    let err = validate_set(lib.dest.path(), "19990101T000000Z").unwrap_err();
    assert!(matches!(err, RestoreError::SetNotFound(_)), "{err}");
}

#[test]
fn overwriting_an_existing_library_needs_explicit_confirmation() {
    let lib = Lib::backed_up();
    let sidecar = sidecar_path(lib.photos().join("a.jpg").to_str().unwrap());
    std::fs::write(&sidecar, xmp("a.jpg", 3)).unwrap();

    let err = restore_set(lib.dest.path(), &lib.set, &lib.opts(false), &RestoreHooks::none())
        .unwrap_err();

    assert!(matches!(err, RestoreError::OverwriteNotConfirmed { count } if count >= 1), "{err}");
    assert_eq!(std::fs::read_to_string(&sidecar).unwrap(), xmp("a.jpg", 3), "nothing touched");
    assert!(leftovers(lib.dir.path()).is_empty());

    restore_set(lib.dest.path(), &lib.set, &lib.opts(true), &RestoreHooks::none()).unwrap();
    assert_eq!(std::fs::read_to_string(&sidecar).unwrap(), xmp("a.jpg", 1));
}

#[test]
fn identical_files_are_left_alone_without_confirmation() {
    let lib = Lib::backed_up();
    let sidecar = sidecar_path(lib.photos().join("c.jpg").to_str().unwrap());
    std::fs::remove_file(lib.catalog_path.clone()).unwrap();
    std::fs::remove_file(sidecar_path(lib.photos().join("a.jpg").to_str().unwrap())).unwrap();

    let report =
        restore_set(lib.dest.path(), &lib.set, &lib.opts(false), &RestoreHooks::none()).unwrap();

    assert!(report.files_unchanged >= 3, "{report:?}");
    assert!(sidecar.exists());
}

#[test]
fn a_failure_mid_restore_rolls_everything_back() {
    let lib = Lib::backed_up();
    // The live library moved on since the backup.
    let a = sidecar_path(lib.photos().join("a.jpg").to_str().unwrap());
    std::fs::write(&a, xmp("a.jpg", 4)).unwrap();
    std::fs::write(&lib.settings, br#"{"language":"nl"}"#).unwrap();
    let cat = Catalog::open(&lib.catalog_path).unwrap();
    cat.conn().execute("DELETE FROM files WHERE filename = 'c.jpg'", []).unwrap();
    drop(cat);
    let before = lib.user_files();
    let catalog_before = std::fs::read(&lib.catalog_path).unwrap();

    let fault = |n: usize| if n == 2 { Fault::Fail } else { Fault::None };
    let hooks = RestoreHooks { fault: &fault, ..RestoreHooks::none() };
    let err = restore_set(lib.dest.path(), &lib.set, &lib.opts(true), &hooks).unwrap_err();

    assert!(matches!(err, RestoreError::Io { .. }), "{err}");
    assert_eq!(lib.user_files(), before, "previous sidecars and settings intact");
    assert_eq!(std::fs::read(&lib.catalog_path).unwrap(), catalog_before);
    assert!(leftovers(lib.dir.path()).is_empty(), "{:?}", leftovers(lib.dir.path()));
}

#[test]
fn a_killed_restore_is_rolled_back_on_next_start() {
    let lib = Lib::backed_up();
    let a = sidecar_path(lib.photos().join("a.jpg").to_str().unwrap());
    std::fs::write(&a, xmp("a.jpg", 4)).unwrap();
    std::fs::write(&lib.settings, br#"{"language":"nl"}"#).unwrap();
    let cat = Catalog::open(&lib.catalog_path).unwrap();
    cat.conn().execute("DELETE FROM files WHERE filename = 'c.jpg'", []).unwrap();
    drop(cat);
    let before = lib.user_files();
    let catalog_before = std::fs::read(&lib.catalog_path).ok();
    let journal_dir = lib.dir.path();

    // The process dies after the second file was swapped in.
    let fault = |n: usize| if n == 2 { Fault::Crash } else { Fault::None };
    let hooks = RestoreHooks { fault: &fault, ..RestoreHooks::none() };
    let err = restore_set(lib.dest.path(), &lib.set, &lib.opts(true), &hooks).unwrap_err();
    assert!(matches!(err, RestoreError::Interrupted), "{err}");
    assert_ne!(lib.user_files(), before, "the crash left a half-swapped library");

    let recovered = recover_interrupted(journal_dir).unwrap();

    assert!(recovered, "a journal was found");
    assert_eq!(lib.user_files(), before, "previous library is back");
    assert_eq!(std::fs::read(&lib.catalog_path).ok(), catalog_before);
    assert!(leftovers(lib.dir.path()).is_empty());
    assert!(!recover_interrupted(journal_dir).unwrap(), "idempotent");
}

#[test]
fn restore_after_the_swap_completes_is_not_rolled_back() {
    let lib = Lib::backed_up();
    lib.wipe();
    restore_set(lib.dest.path(), &lib.set, &lib.opts(false), &RestoreHooks::none()).unwrap();
    let before = lib.user_files();

    assert!(!recover_interrupted(lib.dir.path()).unwrap(), "no journal after success");
    assert_eq!(lib.user_files(), before);
}

#[test]
fn catalog_lands_at_the_requested_location() {
    let lib = Lib::backed_up();
    let moved = tempfile::tempdir().unwrap().keep().join("new-home").join("catalog.db");

    restore_set(
        lib.dest.path(),
        &lib.set,
        &RestoreOptions { catalog_path: moved.clone(), confirm_overwrite: true },
        &RestoreHooks::none(),
    )
    .unwrap();

    let cat = Catalog::open(&moved).unwrap();
    let n: i64 = cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 3);
}

#[test]
fn sidecars_of_folders_that_no_longer_exist_are_skipped_not_invented() {
    let lib = Lib::backed_up();
    lib.wipe();
    std::fs::remove_dir_all(lib.photos()).unwrap();

    let report =
        restore_set(lib.dest.path(), &lib.set, &lib.opts(false), &RestoreHooks::none()).unwrap();

    assert_eq!(report.skipped.len(), 3, "{report:?}");
    assert!(!lib.photos().exists(), "no folder recreated on a possibly unplugged drive");
    assert!(lib.catalog_path.exists() && lib.settings.exists());
}

#[test]
fn cancel_before_the_swap_changes_nothing() {
    let lib = Lib::backed_up();
    lib.wipe();
    let cancelled = || true;
    let hooks = RestoreHooks { cancelled: &cancelled, ..RestoreHooks::none() };

    let err = restore_set(lib.dest.path(), &lib.set, &lib.opts(true), &hooks).unwrap_err();

    assert!(matches!(err, RestoreError::Cancelled), "{err}");
    assert!(!lib.catalog_path.exists());
    assert!(leftovers(lib.dir.path()).is_empty());
}
