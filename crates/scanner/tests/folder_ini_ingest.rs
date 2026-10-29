//! TASK-03-022: folder ini ingest at scan time. A scan only *offers* the
//! migration (never writes without consent); accepting converts the
//! stars, albums and faces to XMP with zero loss; a second scan changes
//! nothing (PLAN §1 onboarding, §6 P2a).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{sidecar_path, Catalog, FolderIniStatus};
use fotobase_metadata::{Sidecar, FOLDER_INI_FILE, FOLDER_INI_HIDDEN_FILE};
use fotobase_scanner::{scan_folder, ScanReport};

const FIXTURE_INI: &str = include_str!("fixtures/folder-ini/sample.ini");

/// Photos of the fixture library. `img_0003.JPG` is on disk under a
/// different case than its `[IMG_0003.jpg]` section; `IMG_0004.JPG` has a
/// section with nothing to migrate.
const PHOTOS: [&str; 4] = ["IMG_0001.JPG", "IMG_0002.JPG", "img_0003.JPG", "IMG_0004.JPG"];

struct Expected {
    photo: &'static str,
    rating: Option<u8>,
    tags: &'static [&'static str],
    caption: Option<&'static str>,
    albums: &'static [&'static str],
    /// (name, x, y, w, h) as worked out by hand from the rect64 values.
    faces: &'static [(&'static str, f64, f64, f64, f64)],
    sidecar: bool,
}

/// Hand-verified result of migrating the fixture (rect64 edges are 16-bit
/// fractions: 0x3333/65535 = 0.2, 0x6666 = 0.4, 0x9999 = 0.6, …).
const EXPECTED: [Expected; 4] = [
    Expected {
        photo: "IMG_0001.JPG",
        rating: Some(5),
        tags: &["rome", "colosseum"],
        caption: Some("Colosseum at dusk"),
        albums: &["Rome trip"],
        faces: &[("Anna Smith", 0.4, 0.25, 0.4, 0.3), ("Marco", 0.45, 0.3, 0.1, 0.2)],
        sidecar: true,
    },
    Expected {
        photo: "IMG_0002.JPG",
        rating: None,
        tags: &[],
        caption: None,
        albums: &["Rome trip"],
        faces: &[],
        sidecar: true,
    },
    Expected {
        photo: "img_0003.JPG",
        rating: Some(5),
        tags: &[],
        caption: None,
        albums: &[],
        faces: &[],
        sidecar: true,
    },
    Expected {
        photo: "IMG_0004.JPG",
        rating: None,
        tags: &[],
        caption: None,
        albums: &[],
        faces: &[],
        sidecar: false,
    },
];

fn make_library(root: &Path) {
    for name in PHOTOS {
        fs::write(root.join(name), format!("fake image bytes of {name}")).unwrap();
    }
    fs::write(root.join(FOLDER_INI_FILE), FIXTURE_INI).unwrap();
}

fn scan(cat: &mut Catalog, root: &Path) -> ScanReport {
    scan_folder(cat, root, &AtomicBool::new(false)).unwrap()
}

fn sidecar_of(root: &Path, photo: &str) -> Option<Vec<u8>> {
    fs::read(sidecar_path(root.join(photo).to_str().unwrap())).ok()
}

fn snapshot(root: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    PHOTOS.iter().map(|p| (p.to_string(), sidecar_of(root, p))).collect()
}

fn check_against_expected(cat: &Catalog, root: &Path) {
    let albums = cat.list_albums().unwrap();
    for e in &EXPECTED {
        let path: PathBuf = root.join(e.photo);
        let sc_path = sidecar_path(path.to_str().unwrap());
        assert_eq!(sc_path.exists(), e.sidecar, "{}: sidecar presence", e.photo);
        if !e.sidecar {
            continue;
        }
        let sc = Sidecar::read(&sc_path).unwrap();
        assert_eq!(sc.rating(), e.rating, "{}: rating", e.photo);
        assert_eq!(sc.tags(), e.tags, "{}: tags", e.photo);
        assert_eq!(sc.caption().as_deref(), e.caption, "{}: caption", e.photo);

        let names: Vec<String> = sc
            .albums()
            .iter()
            .map(|u| albums.iter().find(|a| &a.uuid == u).expect("album exists").name.clone())
            .collect();
        assert_eq!(names, e.albums, "{}: albums", e.photo);

        let faces = sc.face_regions();
        assert_eq!(faces.len(), e.faces.len(), "{}: face count {faces:?}", e.photo);
        for (got, (name, x, y, w, h)) in faces.iter().zip(e.faces) {
            assert_eq!(&got.name, name, "{}", e.photo);
            for (label, a, b) in
                [("x", got.x, *x), ("y", got.y, *y), ("w", got.w, *w), ("h", got.h, *h)]
            {
                assert!((a - b).abs() < 1e-4, "{} {name} {label}: {a} vs {b}", e.photo);
            }
        }
    }

    // The one album, with its definition, and both members.
    assert_eq!(albums.len(), 1, "{albums:?}");
    let rome = &albums[0];
    assert_eq!(rome.name, "Rome trip");
    assert_eq!(rome.date.as_deref(), Some("2009-04-14"));
    assert_eq!(rome.place.as_deref(), Some("Rome"));
    assert_eq!(rome.description.as_deref(), Some("Spring 2009"));
    assert_eq!(rome.count, 2);
}

#[test]
fn a_scan_offers_the_migration_once_and_writes_nothing() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    make_library(lib.path());
    let mut cat = Catalog::open(data.path().join("catalog.db")).unwrap();

    let first = scan(&mut cat, lib.path());
    assert_eq!(first.folder_ini_offered, vec![lib.path().to_str().unwrap().to_string()]);
    assert_eq!(cat.folder_ini_pending_folders().unwrap(), first.folder_ini_offered);
    assert!(snapshot(lib.path()).iter().all(|(_, s)| s.is_none()), "no sidecar without consent");
    assert!(cat.list_albums().unwrap().is_empty());

    let second = scan(&mut cat, lib.path());
    assert!(second.folder_ini_offered.is_empty(), "already offered");
    assert_eq!(cat.folder_ini_pending_folders().unwrap().len(), 1, "still waiting for an answer");
}

#[test]
fn a_folder_without_an_ini_is_never_offered() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    fs::write(lib.path().join("a.jpg"), b"x").unwrap();
    let mut cat = Catalog::open(data.path().join("catalog.db")).unwrap();
    assert!(scan(&mut cat, lib.path()).folder_ini_offered.is_empty());
    assert!(cat.folder_ini_pending_folders().unwrap().is_empty());
}

#[test]
fn nested_folders_are_offered_individually_under_their_own_path() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let sub = lib.path().join("2009").join("Rome");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join(FOLDER_INI_HIDDEN_FILE), "[a.jpg]\nstar=yes\n").unwrap();
    fs::write(sub.join("a.jpg"), b"x").unwrap();
    fs::write(lib.path().join("top.jpg"), b"x").unwrap();
    let mut cat = Catalog::open(data.path().join("catalog.db")).unwrap();

    let report = scan(&mut cat, lib.path());
    assert_eq!(report.folder_ini_offered, vec![sub.to_str().unwrap().to_string()]);
}

#[test]
fn accepting_migrates_with_no_loss_and_a_second_scan_changes_nothing() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    make_library(lib.path());
    let ini_before = fs::read(lib.path().join(FOLDER_INI_FILE)).unwrap();
    let originals: Vec<Vec<u8>> =
        PHOTOS.iter().map(|p| fs::read(lib.path().join(p)).unwrap()).collect();
    let mut cat = Catalog::open(data.path().join("catalog.db")).unwrap();
    scan(&mut cat, lib.path());

    let dir = lib.path().to_str().unwrap();
    let report = cat.ingest_folder_ini(dir).unwrap();
    assert!(report.failed.is_empty(), "{report:?}");
    assert_eq!(report.written, 3);
    check_against_expected(&cat, lib.path());
    assert_eq!(cat.folder_ini_status(dir).unwrap(), Some(FolderIniStatus::Done));

    // Idempotent: another scan and another ingest change nothing.
    let sidecars = snapshot(lib.path());
    let rescan = scan(&mut cat, lib.path());
    assert!(!rescan.changed(), "{rescan:?}");
    assert!(rescan.folder_ini_offered.is_empty());
    let again = cat.ingest_folder_ini(dir).unwrap();
    assert_eq!(again.written, 0, "{again:?}");
    assert_eq!(snapshot(lib.path()), sidecars);
    check_against_expected(&cat, lib.path());

    // The original data is untouched.
    assert_eq!(fs::read(lib.path().join(FOLDER_INI_FILE)).unwrap(), ini_before);
    for (p, bytes) in PHOTOS.iter().zip(&originals) {
        assert_eq!(&fs::read(lib.path().join(p)).unwrap(), bytes, "original {p}");
    }
}

#[test]
fn declining_keeps_the_folder_untouched_and_is_never_offered_again() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    make_library(lib.path());
    let mut cat = Catalog::open(data.path().join("catalog.db")).unwrap();
    scan(&mut cat, lib.path());

    let dir = lib.path().to_str().unwrap();
    cat.decline_folder_ini_ingest(dir).unwrap();
    // A different folder is unaffected: the answer is per folder.
    let other = lib.path().join("other");
    fs::create_dir(&other).unwrap();
    fs::write(other.join(FOLDER_INI_FILE), "[x.jpg]\nstar=yes\n").unwrap();
    fs::write(other.join("x.jpg"), b"x").unwrap();

    let report = scan(&mut cat, lib.path());
    assert_eq!(report.folder_ini_offered, vec![other.to_str().unwrap().to_string()]);
    assert!(snapshot(lib.path()).iter().all(|(_, s)| s.is_none()));
    assert_eq!(cat.folder_ini_status(dir).unwrap(), Some(FolderIniStatus::Declined));
}

#[test]
fn migrated_data_comes_back_after_the_catalog_is_deleted() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    make_library(lib.path());
    let db = data.path().join("catalog.db");
    let mut cat = Catalog::open(&db).unwrap();
    scan(&mut cat, lib.path());
    cat.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();
    drop(cat);

    for ext in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{ext}", db.display()));
    }
    let mut fresh = Catalog::open(&db).unwrap();
    scan(&mut fresh, lib.path());

    let starred = fresh.list_starred(None, 5, usize::MAX).unwrap();
    let mut names: Vec<_> = starred.iter().map(|f| f.filename.clone()).collect();
    names.sort();
    assert_eq!(names, vec!["IMG_0001.JPG", "img_0003.JPG"]);
    check_against_expected(&fresh, lib.path());
}
