//! TASK-03-011: the app-facing half of batch rename / move. The pure
//! planning and file work is covered in the scanner crate; this checks the
//! command layer's contract — preview never touches disk, a plan with
//! collisions is never applied, and moves only go to cataloged folders.

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::Catalog;
use fotobase_lib::batch_commands as cmds;
use fotobase_scanner::batch::{CollisionPolicy, RenameSpec};
use fotobase_scanner::scan_folder;

fn spec(pattern: &str) -> RenameSpec {
    RenameSpec { pattern: pattern.into(), start: 1, step: 1, pad: 2 }
}

/// `root/rel` with native separators, as the scanner stores paths (a mixed
/// `\\` and `/` spelling would not match the catalog row on Windows).
fn at(root: &Path, rel: &str) -> std::path::PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, part| p.join(part))
}

/// Library root with `a/` (two photos) and `b/` (empty), scanned.
fn fixture() -> (tempfile::TempDir, Catalog, Vec<i64>) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("lib");
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(root.join("b")).unwrap();
    fs::write(root.join("a/one.jpg"), b"1").unwrap();
    fs::write(root.join("a/two.jpg"), b"22").unwrap();
    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, &root, &AtomicBool::new(false)).unwrap();
    let ids = ["a/one.jpg", "a/two.jpg"]
        .iter()
        .map(|r| cat.file_id_by_path(at(&root, r).to_str().unwrap()).unwrap().unwrap())
        .collect();
    (tmp, cat, ids)
}

fn lib(tmp: &tempfile::TempDir, rel: &str) -> std::path::PathBuf {
    at(&tmp.path().join("lib"), rel)
}

#[test]
fn preview_rename_lists_the_moves_and_touches_nothing() {
    let (tmp, cat, ids) = fixture();
    let plan =
        cmds::preview_rename(&cat, &ids, &spec("trip-{n}"), CollisionPolicy::Refuse).unwrap();
    assert_eq!(plan.moves.len(), 2);
    assert!(plan.moves[0].to.ends_with("trip-01.jpg"));
    assert!(lib(&tmp, "a/one.jpg").exists() && !lib(&tmp, "a/trip-01.jpg").exists());
}

#[test]
fn rename_applies_and_reports_the_moves() {
    let (tmp, mut cat, ids) = fixture();
    let moves = cmds::rename(&mut cat, &ids, &spec("trip-{n}"), CollisionPolicy::Refuse).unwrap();
    assert_eq!(moves.len(), 2);
    assert!(lib(&tmp, "a/trip-01.jpg").exists() && lib(&tmp, "a/trip-02.jpg").exists());
    assert_eq!(cat.batch_files(&ids).unwrap()[0].path, moves[0].to);
}

#[test]
fn a_malformed_pattern_is_an_error_not_a_partial_rename() {
    let (tmp, mut cat, ids) = fixture();
    let err = cmds::rename(&mut cat, &ids, &spec("{oops}"), CollisionPolicy::Refuse).unwrap_err();
    assert!(err.contains("oops"), "{err}");
    assert!(lib(&tmp, "a/one.jpg").exists());
}

#[test]
fn colliding_plans_are_refused_whole() {
    let (tmp, mut cat, ids) = fixture();
    let err = cmds::rename(&mut cat, &ids, &spec("same"), CollisionPolicy::Refuse).unwrap_err();
    assert!(err.contains("collision"), "{err}");
    assert!(lib(&tmp, "a/one.jpg").exists() && lib(&tmp, "a/two.jpg").exists());

    // Auto-suffix resolves the same selection.
    cmds::rename(&mut cat, &ids, &spec("same"), CollisionPolicy::AutoSuffix).unwrap();
    assert!(lib(&tmp, "a/same.jpg").exists() && lib(&tmp, "a/same (2).jpg").exists());
}

#[test]
fn move_goes_to_a_cataloged_folder_only() {
    let (tmp, mut cat, ids) = fixture();
    // Created after the scan: exists on disk, unknown to the catalog.
    let stranger = lib(&tmp, "c");
    fs::create_dir_all(&stranger).unwrap();
    let err =
        cmds::move_photos(&mut cat, &ids, stranger.to_str().unwrap(), CollisionPolicy::Refuse)
            .unwrap_err();
    assert!(err.contains("not in the library"), "{err}");
    assert!(lib(&tmp, "a/one.jpg").exists());

    let dest = lib(&tmp, "b");
    let moves =
        cmds::move_photos(&mut cat, &ids, dest.to_str().unwrap(), CollisionPolicy::Refuse).unwrap();
    assert_eq!(moves.len(), 2);
    assert!(Path::new(&lib(&tmp, "b/one.jpg")).exists());
    assert!(!lib(&tmp, "a/one.jpg").exists());
}

#[test]
fn preview_move_reports_collisions() {
    let (tmp, cat, ids) = fixture();
    let dest = lib(&tmp, "b");
    fs::write(lib(&tmp, "b/two.jpg"), b"x").unwrap();
    let plan =
        cmds::preview_move(&cat, &ids, dest.to_str().unwrap(), CollisionPolicy::Refuse).unwrap();
    assert_eq!(plan.moves.len(), 1);
    assert_eq!(plan.collisions.len(), 1);
}

// ---- TASK-04-012: pattern rename with date/resolution suffixes -------------

/// Give every cataloged file a known resolution (the fixture bytes are not
/// real images, so the scanner leaves width/height NULL).
fn with_resolution(cat: &Catalog, w: i64, h: i64) {
    cat.conn().execute("UPDATE files SET width = ?1, height = ?2", [w, h]).unwrap();
}

#[test]
fn rename_three_files_with_date_and_resolution_suffixes() {
    let (tmp, mut cat, ids) = fixture();
    assert_eq!(ids.len(), 2);
    fs::write(lib(&tmp, "a/three.jpg"), b"333").unwrap();
    let scan = scan_folder(&mut cat, &tmp.path().join("lib"), &AtomicBool::new(false)).unwrap();
    let _ = scan;
    let three = cat
        .file_id_by_path(at(&tmp.path().join("lib"), "a/three.jpg").to_str().unwrap())
        .unwrap()
        .unwrap();
    let ids = [ids[0], ids[1], three];
    with_resolution(&cat, 1920, 1080);

    let spec = RenameSpec { pattern: "trip-{date}-{res}-{n}".into(), start: 1, step: 1, pad: 2 };
    let moves = cmds::rename(&mut cat, &ids, &spec, CollisionPolicy::Refuse).unwrap();
    assert_eq!(moves.len(), 3);
    let names: Vec<String> = moves
        .iter()
        .map(|m| Path::new(&m.to).file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    // Same date for all three, resolution suffix, numbered — no collisions.
    let d = "2026-11-02";
    assert_eq!(names[0], format!("trip-{d}-1920x1080-01.jpg"), "{names:?}");
    assert_eq!(names[1], format!("trip-{d}-1920x1080-02.jpg"));
    assert_eq!(names[2], format!("trip-{d}-1920x1080-03.jpg"));
    for m in &moves {
        assert!(Path::new(&m.to).is_file(), "{} missing", m.to);
    }
}

#[test]
fn a_taken_target_name_gets_the_next_free_name_never_overwrites() {
    let (tmp, mut cat, ids) = fixture();
    // The target name is already taken by a pre-existing file on disk.
    fs::write(lib(&tmp, "a/trip-01.jpg"), b"pre-existing").unwrap();
    let moves =
        cmds::rename(&mut cat, &ids, &spec("trip-{n}"), CollisionPolicy::AutoSuffix).unwrap();
    assert!(lib(&tmp, "a/trip-01.jpg").exists());
    assert_eq!(fs::read(lib(&tmp, "a/trip-01.jpg")).unwrap(), b"pre-existing");
    assert!(lib(&tmp, "a/trip-01 (2).jpg").exists());
    assert_eq!(moves.len(), 2);
}

#[test]
fn catalog_lookup_by_the_new_name_resolves_the_same_photo() {
    let (_tmp, mut cat, ids) = fixture();
    with_resolution(&cat, 800, 600);
    let spec = RenameSpec { pattern: "pic-{res}-{n}".into(), start: 1, step: 1, pad: 2 };
    let moves = cmds::rename(&mut cat, &ids, &spec, CollisionPolicy::Refuse).unwrap();
    // The row keeps its id (ratings/tags stay attached) and the path follows.
    for m in &moves {
        let found = cat.file_id_by_path(&m.to).unwrap().unwrap();
        assert_eq!(found, m.id, "{} did not resolve to the same photo", m.to);
        let files = cat.batch_files(&[m.id]).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, m.to);
        assert_eq!(files[0].width, Some(800));
    }
}
