//! TASK-03-003: move/rename re-association (00-010 design §4).
//!
//! Unit: the tiered matcher — (known path) → (size + mtime window) → (lazy
//! content hash) — returns the right missing record for rename, move, and
//! both-changed-mtime cases, and `None` for a genuinely new file.
//! Integration: rename/move a fixture file in the OS → rescan → stars/tags
//! intact, sidecar follows the original, no duplicate row; ambiguous matches
//! are surfaced, never silently resolved.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{sidecar_path, Catalog};
use fotobase_metadata::Sidecar;
use fotobase_scanner::{scan_folder, ReassocOutcome, ScanReport};
use tempfile::TempDir;

static CANCEL: AtomicBool = AtomicBool::new(false);

fn open_in(dir: &TempDir) -> Catalog {
    Catalog::open(dir.path().join("catalog.db")).expect("open catalog")
}

fn scan(cat: &mut Catalog, root: &Path) -> ScanReport {
    scan_folder(cat, root, &CANCEL).unwrap()
}

/// A catalogued photo with a rated sidecar, like a real library after use.
fn library_with_rated_photo(
    dir: &TempDir,
) -> (Catalog, i64, std::path::PathBuf, std::path::PathBuf) {
    let mut cat = open_in(dir);
    let root = dir.path().to_path_buf();
    let original = root.join("IMG_0001.jpg");
    std::fs::write(&original, b"identical image bytes").unwrap();
    let sc = sidecar_path(original.to_str().unwrap());
    let mut side = Sidecar::read(&sc).unwrap();
    side.set_rating(5);
    side.write(&sc).unwrap();

    scan(&mut cat, &root);
    // Converge so the sidecar rating lands in the catalog.
    let id: i64 = cat.conn().query_row("SELECT id FROM files", [], |r| r.get(0)).unwrap();
    cat.sync_file(id).unwrap();
    (cat, id, original, sc)
}

fn rating_of_path(cat: &Catalog, path: &Path) -> Option<i64> {
    cat.conn()
        .query_row(
            "SELECT m.rating FROM files f LEFT JOIN metadata m ON m.file_id = f.id WHERE f.path = ?1",
            [path.to_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap()
}

fn row_count(cat: &Catalog) -> i64 {
    cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap()
}

// ---- unit: the tiered matcher ------------------------------------------

fn rec(
    id: i64,
    path: &str,
    size: i64,
    mtime: f64,
    hash: Option<&str>,
) -> fotobase_scanner::MissingFile {
    fotobase_scanner::MissingFile {
        id,
        path: path.to_string(),
        size,
        mtime,
        hash: hash.map(str::to_string),
        media_type: "image".to_string(),
        rating: None,
        caption: None,
        latitude: None,
        longitude: None,
    }
}

const H_A: &str = "aa";
const H_B: &str = "bb";

#[test]
fn matcher_rename_same_size_same_mtime_matches_by_window() {
    let candidates = vec![rec(1, "/old/a.jpg", 100, 1000.0, None)];
    // Hash computation is lazy: never needed when the window tier confirms.
    let outcome = fotobase_scanner::match_missing(&candidates, 100, 1000.5, "image", &|| {
        panic!("hash must stay lazy")
    });
    assert_eq!(outcome, ReassocOutcome::Match(1));
}

#[test]
fn matcher_move_preserves_mtime_so_window_tier_still_matches() {
    let candidates = vec![rec(1, "/old/a.jpg", 100, 1000.0, Some(H_A))];
    let outcome =
        fotobase_scanner::match_missing(&candidates, 100, 1000.2, "image", &|| Some(H_A.into()));
    assert_eq!(outcome, ReassocOutcome::Match(1));
}

#[test]
fn matcher_both_changed_mtime_falls_through_to_hash_tier() {
    let candidates = vec![rec(1, "/old/a.jpg", 100, 1000.0, Some(H_A))];
    // mtime changed (recopied file) → outside the window → hash decides.
    let outcome =
        fotobase_scanner::match_missing(&candidates, 100, 9999.0, "image", &|| Some(H_A.into()));
    assert_eq!(outcome, ReassocOutcome::Match(1));

    // Different content at the same size → no false merge.
    let outcome =
        fotobase_scanner::match_missing(&candidates, 100, 9999.0, "image", &|| Some(H_B.into()));
    assert_eq!(outcome, ReassocOutcome::NoMatch);
}

#[test]
fn matcher_none_for_a_genuinely_new_file() {
    let candidates = vec![rec(1, "/old/a.jpg", 100, 1000.0, Some(H_A))];
    let outcome =
        fotobase_scanner::match_missing(&candidates, 999, 1000.0, "image", &|| Some(H_A.into()));
    assert_eq!(outcome, ReassocOutcome::NoMatch);

    // Right size, wrong media type.
    let outcome =
        fotobase_scanner::match_missing(&candidates, 100, 1000.0, "video", &|| Some(H_A.into()));
    assert_eq!(outcome, ReassocOutcome::NoMatch);
}

#[test]
fn matcher_ambiguous_two_identical_candidates_are_surfaced() {
    let candidates = vec![
        rec(1, "/old/a.jpg", 100, 1000.0, Some(H_A)),
        rec(2, "/old/b.jpg", 100, 1000.0, Some(H_A)),
    ];
    let outcome =
        fotobase_scanner::match_missing(&candidates, 100, 9999.0, "image", &|| Some(H_A.into()));
    assert_eq!(outcome, ReassocOutcome::Ambiguous(2));
}

#[test]
fn matcher_two_window_candidates_disambiguate_by_hash() {
    let candidates = vec![
        rec(1, "/old/a.jpg", 100, 1000.0, Some(H_A)),
        rec(2, "/old/b.jpg", 100, 1000.0, Some(H_B)),
    ];
    // Same size, both in the window → hash decides, no silent pick.
    let outcome =
        fotobase_scanner::match_missing(&candidates, 100, 1000.5, "image", &|| Some(H_B.into()));
    assert_eq!(outcome, ReassocOutcome::Match(2));
}

// ---- integration: rename / move → rescan -------------------------------

#[test]
fn rename_outside_the_app_reassociates_user_data_and_sidecar_follows() {
    let dir = TempDir::new().unwrap();
    let (mut cat, _id, original, old_sc) = library_with_rated_photo(&dir);

    // OS-level rename; the sidecar is deliberately left behind to prove it
    // follows the original (00-010 §4.3).
    let renamed = dir.path().join("renamed.jpg");
    std::fs::rename(&original, &renamed).unwrap();

    let report = scan(&mut cat, dir.path());
    assert_eq!(report.added, 0, "renamed file must not be treated as new");
    assert_eq!(report.reassociated, 1);
    assert_eq!(row_count(&cat), 1, "no duplicate row");

    assert_eq!(rating_of_path(&cat, &renamed), Some(5), "stars survive the rename");
    let new_sc = sidecar_path(renamed.to_str().unwrap());
    assert!(new_sc.exists(), "sidecar follows the original");
    assert!(!old_sc.exists(), "old sidecar location is vacated");
}

#[test]
fn move_to_subfolder_reassociates_user_data_and_sidecar_follows() {
    let dir = TempDir::new().unwrap();
    let (mut cat, _id, original, _old_sc) = library_with_rated_photo(&dir);

    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    let moved = sub.join("IMG_0001.jpg");
    std::fs::rename(&original, &moved).unwrap();

    let report = scan(&mut cat, dir.path());
    assert_eq!(report.added, 0);
    assert_eq!(report.reassociated, 1);
    assert_eq!(row_count(&cat), 1);
    assert_eq!(rating_of_path(&cat, &moved), Some(5));
    assert!(sidecar_path(moved.to_str().unwrap()).exists());
}

#[test]
fn genuinely_new_file_is_not_merged_with_a_missing_record() {
    let dir = TempDir::new().unwrap();
    let (mut cat, _id, original, _sc) = library_with_rated_photo(&dir);
    let old_path = original.to_str().unwrap().to_string();

    // Delete the original and its (moved-along) sidecar: really gone.
    let _ = std::fs::remove_file(&original);
    let _ = std::fs::remove_file(sidecar_path(&old_path));
    scan(&mut cat, dir.path());

    // A different file with coincidentally similar traits arrives.
    let new_file = dir.path().join("other.jpg");
    std::fs::write(&new_file, b"different bytes entirely").unwrap();

    let report = scan(&mut cat, dir.path());
    assert_eq!(report.added, 1, "a genuinely new file is added as new");
    assert_eq!(row_count(&cat), 1);
    assert_ne!(rating_of_path(&cat, &new_file), Some(5));
}

#[test]
fn ambiguous_match_is_surfaced_not_silently_resolved() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let root = dir.path().to_path_buf();

    // Two byte-identical photos, both catalogued, both vanish in one go.
    for name in ["a.jpg", "b.jpg"] {
        let p = root.join(name);
        std::fs::write(&p, b"identical bytes").unwrap();
    }
    scan(&mut cat, &root);
    // Realistic library: the lazy hash backfill has run, so the missing-file
    // records carry content hashes the matcher can compare against.
    use fotobase_scanner::backfill_hashes;
    while backfill_hashes(&mut cat, 16, &CANCEL).unwrap() > 0 {}
    std::fs::remove_file(root.join("a.jpg")).unwrap();
    std::fs::remove_file(root.join("b.jpg")).unwrap();
    let vanished = scan(&mut cat, &root);
    assert_eq!(vanished.removed, 2);

    // One of them "reappears" under a new name — cannot tell which one.
    std::fs::write(root.join("c.jpg"), b"identical bytes").unwrap();
    let report = scan(&mut cat, &root);
    assert_eq!(report.ambiguous, 1, "ambiguity is surfaced in the report");
    assert_eq!(report.added, 1, "ambiguous matches are not silently resolved");
    assert_eq!(row_count(&cat), 1, "the file is added as new");
    assert_eq!(
        cat.conn()
            .query_row("SELECT COUNT(*) FROM missing_files", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2,
        "both candidates survive: no false merge, no silent drop"
    );
}
