//! On-device search + star/tag editing against a catalog source
//! (TASK-09-004, TDD). The mobile app reuses the desktop FTS layer and the
//! desktop write-through path (sidecar first, catalog second); the source
//! may be read-only or written by an external sync client at any time
//! (PLAN §6 Phase 5, §3 rules 1–2, docs/sidecar-merge-on-write.md).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{sidecar_path, Catalog};
use fotobase_metadata::Sidecar;
use fotobase_scanner::CatalogSource;

fn write_photo(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, [0xff, 0xd8, 0xff, 0xd9, 1, 2, 3]).unwrap();
    path
}

fn scan(cat: &mut Catalog, root: &Path) {
    let report = CatalogSource::open(root)
        .unwrap()
        .scan_settled(cat, &AtomicBool::new(false), 3, &|_| {})
        .unwrap();
    assert!(report.requeued.is_empty());
}

fn file_id(cat: &Catalog, path: &Path) -> i64 {
    cat.file_id_by_path(path.to_str().unwrap()).unwrap().expect("photo is catalogued")
}

fn rating_of(cat: &Catalog, id: i64) -> u8 {
    cat.list_files(usize::MAX).unwrap().into_iter().find(|f| f.id == id).unwrap().rating
}

fn search(cat: &Catalog, query: &str) -> Vec<i64> {
    cat.search_files_detailed(query, None, 50).unwrap().into_iter().map(|f| f.id).collect()
}

#[test]
fn search_finds_scanned_photos_and_fresh_tags_through_the_shared_fts_layer() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).unwrap();
    let beach = write_photo(&lib, "beach_sunset.jpg");
    let city = write_photo(&lib, "city_night.jpg");
    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
    scan(&mut cat, &lib);
    let (beach_id, city_id) = (file_id(&cat, &beach), file_id(&cat, &city));

    assert_eq!(search(&cat, "beach"), vec![beach_id]);
    assert!(search(&cat, "holiday").is_empty());

    cat.add_tag(city_id, "holiday").unwrap();
    assert_eq!(search(&cat, "holiday"), vec![city_id]);
}

#[test]
fn star_and_tag_survive_an_app_restart_and_a_catalog_rebuild() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).unwrap();
    let photo = write_photo(&lib, "a.jpg");
    let db = tmp.path().join("catalog.db");

    let id = {
        let mut cat = Catalog::open(&db).unwrap();
        scan(&mut cat, &lib);
        let id = file_id(&cat, &photo);
        cat.set_rating(id, 5).unwrap();
        cat.add_tag(id, "Trips|2024").unwrap();
        id
    };

    // Restart: same catalog file, fresh process state.
    let cat = Catalog::open(&db).unwrap();
    assert_eq!(rating_of(&cat, id), 5);
    assert_eq!(cat.file_tags(id).unwrap(), vec!["Trips|2024".to_string()]);
    assert_eq!(search(&cat, "2024"), vec![id]);

    // The catalog is a cache: a fresh one rebuilt from the sidecar agrees.
    let mut rebuilt = Catalog::open(tmp.path().join("rebuilt.db")).unwrap();
    scan(&mut rebuilt, &lib);
    let rebuilt_id = file_id(&rebuilt, &photo);
    assert_eq!(rating_of(&rebuilt, rebuilt_id), 5);
    assert_eq!(rebuilt.file_tags(rebuilt_id).unwrap(), vec!["Trips|2024".to_string()]);
}

#[test]
fn edit_after_an_external_sync_write_keeps_the_foreign_content() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).unwrap();
    let photo = write_photo(&lib, "a.jpg");
    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
    scan(&mut cat, &lib);
    let id = file_id(&cat, &photo);
    cat.add_tag(id, "mine").unwrap();

    // Another device edits the sidecar and the sync client delivers it.
    let sc_path = sidecar_path(photo.to_str().unwrap());
    let mut foreign = Sidecar::read(&sc_path).unwrap();
    foreign.set_tags(vec!["mine".to_string(), "from-laptop".to_string()]);
    foreign.write(&sc_path).unwrap();

    cat.set_rating(id, 5).unwrap();

    let sc = Sidecar::read(&sc_path).unwrap();
    assert_eq!(sc.rating(), Some(5));
    assert_eq!(sc.tags(), vec!["mine".to_string(), "from-laptop".to_string()]);
    assert_eq!(cat.file_tags(id).unwrap(), vec!["from-laptop".to_string(), "mine".to_string()]);
}

#[cfg(unix)]
mod read_only {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn set_mode(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    /// Root ignores directory permissions; the safe-fail cannot be observed.
    fn can_write_into(dir: &Path) -> bool {
        let probe = dir.join(".probe");
        let ok = fs::write(&probe, b"x").is_ok();
        let _ = fs::remove_file(&probe);
        ok
    }

    #[test]
    fn edits_fail_cleanly_and_change_nothing_when_the_source_is_read_only() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("lib");
        fs::create_dir(&lib).unwrap();
        let photo = write_photo(&lib, "a.jpg");
        let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
        scan(&mut cat, &lib);
        let id = file_id(&cat, &photo);
        cat.add_tag(id, "keep").unwrap();
        let sc_path = sidecar_path(photo.to_str().unwrap());
        let sidecar_before = fs::read(&sc_path).unwrap();

        set_mode(&lib, 0o555);
        if can_write_into(&lib) {
            set_mode(&lib, 0o755);
            return;
        }
        let entries_before: Vec<_> =
            fs::read_dir(&lib).unwrap().flatten().map(|e| e.path()).collect();

        let star = cat.set_rating(id, 5);
        let add = cat.add_tag(id, "new");
        let remove = cat.remove_tag(id, "keep");

        set_mode(&lib, 0o755);
        assert!(star.is_err() && add.is_err() && remove.is_err());
        assert_eq!(fs::read(&sc_path).unwrap(), sidecar_before, "sidecar untouched");
        let mut entries_after: Vec<_> =
            fs::read_dir(&lib).unwrap().flatten().map(|e| e.path()).collect();
        let mut entries_before = entries_before;
        entries_before.sort();
        entries_after.sort();
        assert_eq!(entries_after, entries_before, "no temp or backup files left behind");
        assert_eq!(rating_of(&cat, id), 0, "catalog only follows a successful sidecar write");
        assert_eq!(cat.file_tags(id).unwrap(), vec!["keep".to_string()]);
        assert!(search(&cat, "new").is_empty());
    }
}
