//! TASK-03-011: batch rename and batch move (PLAN §2 B, 00-010 §4.5).
//!
//! Unit: pattern rendering matches expected names; collision detection flags
//! a fixture with a clashing target.
//! Integration: batch move/rename → files + sidecars at new paths, catalog
//! rows point to new paths, stars intact, contents unchanged, nothing
//! re-scanned as new. An aborted batch is rolled back and reported; the
//! half-applied state a hard kill leaves is repaired by the 03-003
//! re-association safety net.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{sidecar_path, Catalog};
use fotobase_metadata::Sidecar;
use fotobase_scanner::batch::{
    apply_batch, plan_move, plan_rename, render_name, revert_batch, BatchError, BatchPlan,
    CollisionPolicy, CollisionReason, PatternError, RenameSpec,
};
use fotobase_scanner::{scan_folder, ScanReport};
use tempfile::TempDir;

static CANCEL: AtomicBool = AtomicBool::new(false);

fn spec(pattern: &str) -> RenameSpec {
    RenameSpec { pattern: pattern.to_string(), start: 1, step: 1, pad: 0 }
}

fn scan(cat: &mut Catalog, root: &Path) -> ScanReport {
    scan_folder(cat, root, &CANCEL).unwrap()
}

/// `root/rel` with native separators, as the scanner stores paths (a mixed
/// `\\` and `/` spelling would not match the catalog row on Windows).
fn at(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, part| p.join(part))
}

fn path_str(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// Write `name` under `dir` with a rated sidecar, like a library after use.
fn photo(dir: &Path, name: &str, bytes: &[u8], rating: u8) -> PathBuf {
    let original = dir.join(name);
    std::fs::write(&original, bytes).unwrap();
    let sc = sidecar_path(path_str(&original));
    let mut side = Sidecar::read(&sc).unwrap();
    side.set_rating(rating);
    side.write(&sc).unwrap();
    original
}

struct Lib {
    _tmp: TempDir,
    root: PathBuf,
    cat: Catalog,
}

/// Root with `a/` holding IMG_1..IMG_3 (ratings 5, 4, 3) and an empty `b/`.
fn lib() -> Lib {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("lib");
    std::fs::create_dir_all(root.join("a")).unwrap();
    std::fs::create_dir_all(root.join("b")).unwrap();
    for (i, rating) in [(1u8, 5u8), (2, 4), (3, 3)] {
        photo(&root.join("a"), &format!("IMG_{i}.jpg"), format!("bytes {i}").as_bytes(), rating);
    }
    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
    scan(&mut cat, &root);
    let ids: Vec<i64> = {
        let mut stmt = cat.conn().prepare("SELECT id FROM files").unwrap();
        stmt.query_map([], |r| r.get(0)).unwrap().flatten().collect()
    };
    for id in ids {
        cat.sync_file(id).unwrap();
    }
    Lib { _tmp: tmp, root, cat }
}

impl Lib {
    fn id(&self, rel: &str) -> i64 {
        let p = at(&self.root, rel);
        self.cat.file_id_by_path(path_str(&p)).unwrap().unwrap_or_else(|| panic!("no row {rel}"))
    }

    fn ids(&self, rels: &[&str]) -> Vec<i64> {
        rels.iter().map(|r| self.id(r)).collect()
    }

    fn rating(&self, rel: &str) -> Option<i64> {
        let p = at(&self.root, rel);
        self.cat
            .conn()
            .query_row(
                "SELECT m.rating FROM files f LEFT JOIN metadata m ON m.file_id = f.id
                 WHERE f.path = ?1",
                [path_str(&p)],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn row_count(&self) -> i64 {
        self.cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap()
    }

    fn plan_rename(&self, rels: &[&str], s: &RenameSpec, policy: CollisionPolicy) -> BatchPlan {
        let items = self.cat.batch_files(&self.ids(rels)).unwrap();
        plan_rename(&items, s, policy).unwrap()
    }

    fn plan_move(&self, rels: &[&str], dest: &str, policy: CollisionPolicy) -> BatchPlan {
        let items = self.cat.batch_files(&self.ids(rels)).unwrap();
        plan_move(&items, path_str(&self.root.join(dest)), policy)
    }
}

fn names(plan: &BatchPlan) -> Vec<String> {
    plan.moves
        .iter()
        .map(|m| Path::new(&m.to).file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

// ---- unit: pattern engine ------------------------------------------------

#[test]
fn counter_honours_start_step_and_pad() {
    let s = RenameSpec { pattern: "trip-{n}".into(), start: 10, step: 5, pad: 3 };
    assert_eq!(render_name(&s, "IMG_1", 0, None, 0.0).unwrap(), "trip-010");
    assert_eq!(render_name(&s, "IMG_1", 2, None, 0.0).unwrap(), "trip-020");
    assert_eq!(render_name(&spec("name-{n}"), "x", 1, None, 0.0).unwrap(), "name-2");
}

#[test]
fn stem_and_date_tokens_render() {
    let s = spec("{date}_{stem}_{n}");
    let taken = Some("2023-05-01T10:20:30");
    assert_eq!(render_name(&s, "IMG_7", 0, taken, 0.0).unwrap(), "2023-05-01_IMG_7_1");
    // EXIF-style stamps and a missing taken_at (falls back to mtime, UTC).
    assert_eq!(
        render_name(&spec("{date}"), "x", 0, Some("2023:05:01 10:20:30"), 0.0).unwrap(),
        "2023-05-01"
    );
    assert_eq!(
        render_name(&spec("{date}"), "x", 0, None, 86_400.0 * 19_723.0).unwrap(),
        "2024-01-01"
    );
}

#[test]
fn bad_patterns_are_rejected() {
    for bad in ["{nope}", "a{n", "a}b", "", "{n}/x", "a\\b", "a:b", "x*", "..", " "] {
        assert!(
            matches!(render_name(&spec(bad), "x", 0, None, 0.0), Err(PatternError::Invalid(_))),
            "{bad:?} must be rejected"
        );
    }
}

#[test]
fn rename_plan_keeps_extension_and_selection_order() {
    let l = lib();
    let plan =
        l.plan_rename(&["a/IMG_2.jpg", "a/IMG_1.jpg"], &spec("trip-{n}"), CollisionPolicy::Refuse);
    assert_eq!(names(&plan), ["trip-1.jpg", "trip-2.jpg"]);
    assert_eq!(plan.moves[0].id, l.id("a/IMG_2.jpg"));
    assert!(plan.collisions.is_empty());
}

#[test]
fn unchanged_names_are_left_out_of_the_plan() {
    let l = lib();
    let plan = l.plan_rename(&["a/IMG_1.jpg"], &spec("{stem}"), CollisionPolicy::Refuse);
    assert!(plan.moves.is_empty() && plan.collisions.is_empty());
}

// ---- unit: collisions ------------------------------------------------------

#[test]
fn duplicate_targets_are_flagged() {
    let l = lib();
    let plan =
        l.plan_rename(&["a/IMG_1.jpg", "a/IMG_2.jpg"], &spec("same"), CollisionPolicy::Refuse);
    assert!(plan.collisions.iter().any(|c| c.reason == CollisionReason::DuplicateTarget));
}

#[test]
fn existing_file_and_stale_sidecar_at_the_target_are_flagged() {
    let l = lib();
    std::fs::write(l.root.join("a/taken.jpg"), b"other").unwrap();
    let plan = l.plan_rename(&["a/IMG_1.jpg"], &spec("taken"), CollisionPolicy::Refuse);
    assert_eq!(plan.collisions.len(), 1);
    assert_eq!(plan.collisions[0].reason, CollisionReason::TargetExists);

    std::fs::write(l.root.join("a/orphan.jpg.xmp"), b"<x/>").unwrap();
    let plan = l.plan_rename(&["a/IMG_1.jpg"], &spec("orphan"), CollisionPolicy::Refuse);
    assert_eq!(plan.collisions[0].reason, CollisionReason::SidecarExists);
}

#[test]
fn a_target_that_is_another_batch_sources_name_is_flagged() {
    let l = lib();
    // IMG_2 → IMG_1 would clobber the file IMG_1 (which itself moves on).
    let plan =
        l.plan_rename(&["a/IMG_2.jpg", "a/IMG_1.jpg"], &spec("IMG_{n}"), CollisionPolicy::Refuse);
    assert!(plan.collisions.iter().any(|c| c.reason == CollisionReason::TargetExists));
}

#[test]
fn auto_suffix_resolves_clashes_without_touching_other_files() {
    let l = lib();
    std::fs::write(l.root.join("a/same.jpg"), b"other").unwrap();
    let plan =
        l.plan_rename(&["a/IMG_1.jpg", "a/IMG_2.jpg"], &spec("same"), CollisionPolicy::AutoSuffix);
    assert!(plan.collisions.is_empty());
    assert_eq!(names(&plan), ["same (2).jpg", "same (3).jpg"]);
}

#[test]
fn case_only_rename_is_not_a_collision() {
    let l = lib();
    let plan = l.plan_rename(&["a/IMG_1.jpg"], &spec("img_1"), CollisionPolicy::Refuse);
    assert!(plan.collisions.is_empty());
    assert_eq!(names(&plan), ["img_1.jpg"]);
}

#[test]
fn move_collision_with_a_file_in_the_destination_is_flagged() {
    let l = lib();
    std::fs::write(l.root.join("b/IMG_2.jpg"), b"other").unwrap();
    let plan = l.plan_move(&["a/IMG_1.jpg", "a/IMG_2.jpg"], "b", CollisionPolicy::Refuse);
    assert_eq!(plan.collisions.len(), 1);
    assert_eq!(plan.collisions[0].id, l.id("a/IMG_2.jpg"));

    let plan = l.plan_move(&["a/IMG_1.jpg", "a/IMG_2.jpg"], "b", CollisionPolicy::AutoSuffix);
    assert_eq!(names(&plan), ["IMG_1.jpg", "IMG_2 (2).jpg"]);
}

#[test]
fn moving_into_the_current_folder_is_a_no_op() {
    let l = lib();
    let plan = l.plan_move(&["a/IMG_1.jpg"], "a", CollisionPolicy::Refuse);
    assert!(plan.moves.is_empty() && plan.collisions.is_empty());
}

// ---- integration: apply ----------------------------------------------------

#[test]
fn batch_rename_moves_files_sidecars_and_catalog_rows() {
    let mut l = lib();
    let before = std::fs::read(l.root.join("a/IMG_2.jpg")).unwrap();
    let ids = l.ids(&["a/IMG_1.jpg", "a/IMG_2.jpg", "a/IMG_3.jpg"]);
    let plan = l.plan_rename(
        &["a/IMG_1.jpg", "a/IMG_2.jpg", "a/IMG_3.jpg"],
        &spec("trip-{n}"),
        CollisionPolicy::Refuse,
    );

    let applied = apply_batch(&mut l.cat, &plan.moves).unwrap();
    assert_eq!(applied.len(), 3);

    for (i, rating) in [(1, 5), (2, 4), (3, 3)] {
        let new = l.root.join(format!("a/trip-{i}.jpg"));
        assert!(new.exists());
        assert!(sidecar_path(path_str(&new)).exists(), "sidecar follows the original");
        assert!(!l.root.join(format!("a/IMG_{i}.jpg")).exists());
        assert!(!l.root.join(format!("a/IMG_{i}.jpg.xmp")).exists());
        assert_eq!(l.rating(&format!("a/trip-{i}.jpg")), Some(rating), "stars intact");
        let side = Sidecar::read(&sidecar_path(path_str(&new))).unwrap();
        assert_eq!(side.rating(), Some(rating as u8));
    }
    // Same rows (ids), new names; contents untouched.
    assert_eq!(l.ids(&["a/trip-1.jpg", "a/trip-2.jpg", "a/trip-3.jpg"]), ids);
    assert_eq!(std::fs::read(l.root.join("a/trip-2.jpg")).unwrap(), before);
    let name: String = l
        .cat
        .conn()
        .query_row("SELECT filename FROM files WHERE id = ?1", [ids[0]], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "trip-1.jpg");
    assert_eq!(l.cat.search_files("trip", 10).unwrap().len(), 3, "FTS follows the rename");

    // Nothing is re-scanned as new.
    let report = scan(&mut l.cat, &l.root);
    assert_eq!((report.added, report.removed, report.reassociated), (0, 0, 0));
    assert_eq!(report.skipped, 3);
    assert_eq!(l.row_count(), 3);
}

#[test]
fn batch_move_relocates_files_sidecars_and_folder_membership() {
    let mut l = lib();
    let ids = l.ids(&["a/IMG_1.jpg", "a/IMG_3.jpg"]);
    let before = std::fs::read(l.root.join("a/IMG_3.jpg")).unwrap();
    let plan = l.plan_move(&["a/IMG_1.jpg", "a/IMG_3.jpg"], "b", CollisionPolicy::Refuse);
    apply_batch(&mut l.cat, &plan.moves).unwrap();

    for rel in ["IMG_1.jpg", "IMG_3.jpg"] {
        assert!(l.root.join("b").join(rel).exists());
        assert!(l.root.join("b").join(format!("{rel}.xmp")).exists());
        assert!(!l.root.join("a").join(rel).exists());
        assert!(!l.root.join("a").join(format!("{rel}.xmp")).exists());
    }
    assert_eq!(l.ids(&["b/IMG_1.jpg", "b/IMG_3.jpg"]), ids, "rows keep their ids");
    assert_eq!(l.rating("b/IMG_1.jpg"), Some(5));
    assert_eq!(std::fs::read(l.root.join("b/IMG_3.jpg")).unwrap(), before);
    let folder: String = l
        .cat
        .conn()
        .query_row(
            "SELECT d.path FROM files f JOIN folders d ON d.id = f.folder_id WHERE f.id = ?1",
            [ids[0]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(folder, path_str(&l.root.join("b")));

    let report = scan(&mut l.cat, &l.root);
    assert_eq!((report.added, report.removed, report.reassociated), (0, 0, 0));
    assert_eq!(l.row_count(), 3);
}

#[test]
fn batch_without_sidecars_still_moves() {
    let mut l = lib();
    std::fs::remove_file(l.root.join("a/IMG_1.jpg.xmp")).unwrap();
    let plan = l.plan_move(&["a/IMG_1.jpg"], "b", CollisionPolicy::Refuse);
    apply_batch(&mut l.cat, &plan.moves).unwrap();
    assert!(l.root.join("b/IMG_1.jpg").exists());
    assert!(!l.root.join("b/IMG_1.jpg.xmp").exists());
}

#[test]
fn aborted_batch_is_rolled_back_and_reported() {
    let mut l = lib();
    let plan = l.plan_rename(
        &["a/IMG_1.jpg", "a/IMG_2.jpg", "a/IMG_3.jpg"],
        &spec("trip-{n}"),
        CollisionPolicy::Refuse,
    );
    // The third source vanishes between preview and apply.
    std::fs::remove_file(l.root.join("a/IMG_3.jpg")).unwrap();

    let err = apply_batch(&mut l.cat, &plan.moves).unwrap_err();
    let BatchError::Aborted { failed, rolled_back, rollback_failures, .. } = err;
    assert_eq!(failed.id, plan.moves[2].id);
    assert_eq!(rolled_back, 2);
    assert!(rollback_failures.is_empty());

    // First two are back exactly where they were, with sidecars and rows.
    for i in [1, 2] {
        assert!(l.root.join(format!("a/IMG_{i}.jpg")).exists());
        assert!(l.root.join(format!("a/IMG_{i}.jpg.xmp")).exists());
        assert!(!l.root.join(format!("a/trip-{i}.jpg")).exists());
        assert!(!l.root.join(format!("a/trip-{i}.jpg.xmp")).exists());
    }
    assert_eq!(l.rating("a/IMG_1.jpg"), Some(5));
    assert_eq!(l.rating("a/IMG_2.jpg"), Some(4));
    let moved: i64 = l
        .cat
        .conn()
        .query_row("SELECT COUNT(*) FROM files WHERE path LIKE '%trip-%'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(moved, 0, "no half-moved rows");
}

#[test]
fn applying_over_an_existing_target_is_refused_not_clobbered() {
    let mut l = lib();
    let plan = l.plan_rename(&["a/IMG_1.jpg"], &spec("late"), CollisionPolicy::Refuse);
    std::fs::write(l.root.join("a/late.jpg"), b"precious").unwrap();

    assert!(matches!(apply_batch(&mut l.cat, &plan.moves), Err(BatchError::Aborted { .. })));
    assert_eq!(std::fs::read(l.root.join("a/late.jpg")).unwrap(), b"precious");
    assert!(l.root.join("a/IMG_1.jpg").exists());
}

#[test]
fn applied_moves_can_be_reverted() {
    let mut l = lib();
    let plan = l.plan_move(&["a/IMG_1.jpg", "a/IMG_2.jpg"], "b", CollisionPolicy::Refuse);
    let applied = apply_batch(&mut l.cat, &plan.moves).unwrap();

    revert_batch(&mut l.cat, &applied).unwrap();
    for i in [1, 2] {
        assert!(l.root.join(format!("a/IMG_{i}.jpg")).exists());
        assert!(l.root.join(format!("a/IMG_{i}.jpg.xmp")).exists());
        assert!(!l.root.join(format!("b/IMG_{i}.jpg")).exists());
    }
    assert_eq!(l.rating("a/IMG_1.jpg"), Some(5));
    let report = scan(&mut l.cat, &l.root);
    assert_eq!((report.added, report.removed, report.reassociated), (0, 0, 0));
}

#[test]
fn move_into_a_missing_folder_is_refused() {
    let mut l = lib();
    let plan = l.plan_move(&["a/IMG_1.jpg"], "nowhere", CollisionPolicy::Refuse);
    assert!(matches!(apply_batch(&mut l.cat, &plan.moves), Err(BatchError::Aborted { .. })));
    assert!(l.root.join("a/IMG_1.jpg").exists());
}

// ---- integration: hard-kill safety net ---------------------------------

#[test]
fn half_applied_state_after_a_kill_is_repaired_by_reassociation() {
    let mut l = lib();
    // A kill between "original renamed" and "sidecar renamed + catalog
    // updated": the original sits at the new path, everything else is old.
    std::fs::rename(l.root.join("a/IMG_1.jpg"), l.root.join("b/IMG_1.jpg")).unwrap();

    let report = scan(&mut l.cat, &l.root);
    assert_eq!(report.reassociated, 1, "treated as a move, not a new photo");
    assert_eq!(report.added, 0);
    assert!(l.root.join("b/IMG_1.jpg.xmp").exists(), "orphaned sidecar follows the original");
    assert!(!l.root.join("a/IMG_1.jpg.xmp").exists());
    assert_eq!(l.rating("b/IMG_1.jpg"), Some(5));
    assert_eq!(l.row_count(), 3);
    let name: String = l
        .cat
        .conn()
        .query_row(
            "SELECT filename FROM files WHERE path = ?1",
            [path_str(&at(&l.root, "b/IMG_1.jpg"))],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(name, "IMG_1.jpg");
}

#[test]
fn reassociated_rename_gets_the_new_filename() {
    let mut l = lib();
    std::fs::rename(l.root.join("a/IMG_1.jpg"), l.root.join("a/renamed.jpg")).unwrap();
    std::fs::rename(l.root.join("a/IMG_1.jpg.xmp"), l.root.join("a/renamed.jpg.xmp")).unwrap();
    assert_eq!(scan(&mut l.cat, &l.root).reassociated, 1);
    let name: String = l
        .cat
        .conn()
        .query_row(
            "SELECT filename FROM files WHERE path = ?1",
            [path_str(&at(&l.root, "a/renamed.jpg"))],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(name, "renamed.jpg", "grid and search show the file's current name");
}
