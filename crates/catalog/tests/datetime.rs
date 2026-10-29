//! TASK-03-019: date/time adjustment — shift, timezone fix and multi-camera
//! merge write `exif:DateTimeOriginal` through the sidecar (00-010 §3), the
//! catalog's `files.taken_at` follows, and the capture-time sort sees it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use fotobase_catalog::{sidecar_path, Catalog, FileQuery, FileSort, SyncError, TimeAdjustment};
use fotobase_metadata::Sidecar;

fn add_photo(
    cat: &mut Catalog,
    dir: &Path,
    name: &str,
    taken_at: Option<&str>,
    mtime: f64,
) -> (i64, PathBuf) {
    let original = dir.join(name);
    std::fs::write(&original, b"fake image bytes").unwrap();
    let folder_id = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, taken_at, media_type)
             VALUES (?1, ?2, ?3, 10, ?4, ?5, 'image')",
            rusqlite::params![folder_id, original.to_str().unwrap(), name, mtime, taken_at],
        )
        .unwrap();
    (cat.conn().last_insert_rowid(), original)
}

fn taken_at(cat: &Catalog, id: i64) -> Option<String> {
    cat.conn().query_row("SELECT taken_at FROM files WHERE id = ?1", [id], |r| r.get(0)).unwrap()
}

fn sidecar_time(original: &Path) -> Option<String> {
    Sidecar::read(&sidecar_path(original.to_str().unwrap()))
        .unwrap()
        .capture_time()
        .map(|t| t.to_xmp())
}

fn adjust(cat: &mut Catalog, ids: &[i64], adj: &TimeAdjustment) -> fotobase_catalog::AdjustReport {
    cat.adjust_capture_times(ids, adj, &AtomicBool::new(false), &|_, _| {}).unwrap()
}

fn shift(seconds: i64) -> TimeAdjustment {
    TimeAdjustment::Shift { seconds }
}

fn chrono_order(cat: &Catalog) -> Vec<String> {
    let q = FileQuery { sort: FileSort::Capture, ..FileQuery::default() };
    cat.query_files(&q, usize::MAX).unwrap().into_iter().map(|f| f.filename).collect()
}

#[test]
fn shifting_a_plus_two_photo_back_two_hours_writes_the_sidecar_then_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) =
        add_photo(&mut cat, dir.path(), "a.jpg", Some("2024-06-01T12:00:00+02:00"), 0.0);

    let report = adjust(&mut cat, &[id], &shift(-2 * 3600));

    assert_eq!(report.adjusted, 1);
    assert!(report.failed.is_empty(), "{report:?}");
    assert_eq!(sidecar_time(&original).as_deref(), Some("2024-06-01T10:00:00+02:00"));
    assert_eq!(taken_at(&cat, id).as_deref(), Some("2024-06-01T10:00:00+02:00"));
}

#[test]
fn the_sidecar_time_is_the_base_not_the_catalog_cache() {
    // Another app rewrote the date since the last sync: the sidecar wins.
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg", Some("2000-01-01T00:00:00"), 0.0);
    let sc_path = sidecar_path(original.to_str().unwrap());
    let mut sc = Sidecar::read(&sc_path).unwrap();
    sc.set_capture_time(&fotobase_metadata::CaptureTime::parse("2024-06-01T12:00:00").unwrap());
    sc.write(&sc_path).unwrap();

    adjust(&mut cat, &[id], &shift(60));

    assert_eq!(sidecar_time(&original).as_deref(), Some("2024-06-01T12:01:00"));
}

#[test]
fn exif_style_cached_dates_are_accepted_and_stored_as_iso() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg", Some("2024:06:01 12:00:00"), 0.0);

    adjust(&mut cat, &[id], &shift(3600));

    assert_eq!(taken_at(&cat, id).as_deref(), Some("2024-06-01T13:00:00"));
}

#[test]
fn a_photo_without_any_capture_time_starts_from_its_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    // 2024-06-01T12:00:00Z
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg", None, 1_717_243_200.0);

    adjust(&mut cat, &[id], &shift(-86_400));

    assert_eq!(sidecar_time(&original).as_deref(), Some("2024-05-31T12:00:00+00:00"));
}

#[test]
fn relabelling_the_offset_keeps_the_wall_clock() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) =
        add_photo(&mut cat, dir.path(), "a.jpg", Some("2024-06-01T12:00:00+00:00"), 0.0);

    adjust(&mut cat, &[id], &TimeAdjustment::RelabelOffset { offset_minutes: 120 });

    assert_eq!(sidecar_time(&original).as_deref(), Some("2024-06-01T12:00:00+02:00"));
    assert_eq!(taken_at(&cat, id).as_deref(), Some("2024-06-01T12:00:00+02:00"));
}

#[test]
fn converting_a_camera_left_on_utc_keeps_the_instant() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg", Some("2024-06-01T08:00:00"), 0.0);

    adjust(&mut cat, &[id], &TimeAdjustment::ConvertOffset { offset_minutes: 120 });

    assert_eq!(sidecar_time(&original).as_deref(), Some("2024-06-01T10:00:00+02:00"));
}

#[test]
fn other_sidecar_fields_survive_an_adjustment() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg", Some("2024-06-01T12:00:00"), 0.0);
    cat.set_rating(id, 5).unwrap();
    cat.add_tag(id, "sea").unwrap();

    adjust(&mut cat, &[id], &shift(60));

    let sc = Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap();
    assert_eq!(sc.rating(), Some(5));
    assert_eq!(sc.tags(), vec!["sea"]);
    assert!(
        fotobase_metadata::backup_path(&sidecar_path(original.to_str().unwrap())).exists(),
        "previous packet kept as .fb-bak"
    );
}

#[test]
fn one_bad_photo_is_reported_and_the_rest_still_adjust() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (bad, bad_original) =
        add_photo(&mut cat, dir.path(), "bad.jpg", Some("sometime in june"), 0.0);
    let (late, _) = add_photo(&mut cat, dir.path(), "late.jpg", Some("9999-12-31T23:59:59"), 0.0);
    let (good, good_original) =
        add_photo(&mut cat, dir.path(), "good.jpg", Some("2024-06-01T12:00:00"), 0.0);

    let report = adjust(&mut cat, &[bad, late, good], &shift(3600));

    assert_eq!(report.adjusted, 1);
    let failed: Vec<i64> = report.failed.iter().map(|f| f.id).collect();
    assert_eq!(failed, vec![bad, late]);
    assert!(report.failed.iter().all(|f| !f.error.is_empty()));
    assert!(!sidecar_path(bad_original.to_str().unwrap()).exists(), "no sidecar for a failure");
    assert_eq!(taken_at(&cat, bad).as_deref(), Some("sometime in june"));
    assert_eq!(sidecar_time(&good_original).as_deref(), Some("2024-06-01T13:00:00"));
}

#[test]
fn an_unknown_id_is_a_failed_entry_not_an_abort() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (good, _) = add_photo(&mut cat, dir.path(), "good.jpg", Some("2024-06-01T12:00:00"), 0.0);

    let report = adjust(&mut cat, &[9999, good], &shift(1));

    assert_eq!(report.adjusted, 1);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].id, 9999);
}

#[test]
fn an_invalid_offset_is_refused_before_anything_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg", Some("2024-06-01T12:00:00"), 0.0);

    let err = cat
        .adjust_capture_times(
            &[id],
            &TimeAdjustment::RelabelOffset { offset_minutes: 24 * 60 },
            &AtomicBool::new(false),
            &|_, _| {},
        )
        .unwrap_err();

    assert!(matches!(err, SyncError::Adjust(_)), "{err:?}");
    assert!(!sidecar_path(original.to_str().unwrap()).exists());
}

#[test]
fn progress_reports_every_photo_and_cancel_stops_the_batch() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let ids: Vec<i64> = (0..4)
        .map(|i| {
            add_photo(&mut cat, dir.path(), &format!("{i}.jpg"), Some("2024-06-01T12:00:00"), 0.0).0
        })
        .collect();

    let seen = std::cell::RefCell::new(Vec::new());
    let cancel = AtomicBool::new(false);
    let report = cat
        .adjust_capture_times(&ids, &shift(60), &cancel, &|done, total| {
            seen.borrow_mut().push((done, total));
            if done == 2 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();

    assert_eq!(*seen.borrow(), vec![(1, 4), (2, 4)]);
    assert!(report.cancelled);
    assert_eq!(report.adjusted, 2);
    assert_eq!(taken_at(&cat, ids[1]).as_deref(), Some("2024-06-01T12:01:00"));
    assert_eq!(taken_at(&cat, ids[2]).as_deref(), Some("2024-06-01T12:00:00"), "not reached");
}

#[test]
fn preview_shows_before_and_after_without_touching_disk() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (a, a_original) =
        add_photo(&mut cat, dir.path(), "a.jpg", Some("2024-06-01T12:00:00+02:00"), 0.0);
    let (bad, _) = add_photo(&mut cat, dir.path(), "bad.jpg", Some("???"), 0.0);

    let rows = cat.preview_capture_times(&[a, bad, 4242], &shift(-2 * 3600)).unwrap();

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].id, a);
    assert_eq!(rows[0].before.as_deref(), Some("2024-06-01T12:00:00+02:00"));
    assert_eq!(rows[0].after.as_deref(), Some("2024-06-01T10:00:00+02:00"));
    assert!(rows[0].error.is_none());
    assert!(rows[1].after.is_none() && rows[1].error.is_some());
    assert!(rows[2].error.is_some());
    assert!(!sidecar_path(a_original.to_str().unwrap()).exists());
    assert_eq!(taken_at(&cat, a).as_deref(), Some("2024-06-01T12:00:00+02:00"));
}

#[test]
fn capture_sort_orders_by_instant_across_offsets_then_mtime_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    // Instants: late 11:00Z, early 10:00Z (12:00+02:00), exif 10:30Z (naive =
    // UTC), mtime-only 10:45Z.
    add_photo(&mut cat, dir.path(), "a-late.jpg", Some("2024-06-01T11:00:00+00:00"), 0.0);
    add_photo(&mut cat, dir.path(), "b-early.jpg", Some("2024-06-01T12:00:00+02:00"), 0.0);
    add_photo(&mut cat, dir.path(), "c-exif.jpg", Some("2024:06:01 10:30:00"), 0.0);
    add_photo(&mut cat, dir.path(), "d-mtime.jpg", None, 1_717_238_700.0);

    assert_eq!(chrono_order(&cat), vec!["b-early.jpg", "c-exif.jpg", "d-mtime.jpg", "a-late.jpg"]);
    // The default order is untouched.
    let by_path: Vec<String> = cat
        .query_files(&FileQuery::default(), usize::MAX)
        .unwrap()
        .into_iter()
        .map(|f| f.filename)
        .collect();
    assert_eq!(by_path, vec!["a-late.jpg", "b-early.jpg", "c-exif.jpg", "d-mtime.jpg"]);
}

#[test]
fn merging_two_cameras_interleaves_their_photos_chronologically() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let mut shoot = |name: &str, time: &str| {
        add_photo(
            &mut cat,
            dir.path(),
            &format!("{name}.jpg"),
            Some(&format!("2024-06-01T{time}")),
            0.0,
        )
        .0
    };
    // Camera A is right. Camera B runs 1 h 0 min 30 s behind; a1 and b1 are
    // the same moment (the calibration frame).
    for (name, time) in [("a1", "10:00:00"), ("a2", "10:10:00"), ("a3", "10:20:00")] {
        shoot(name, time);
    }
    let b = [shoot("b1", "08:59:30"), shoot("b2", "09:04:30"), shoot("b3", "09:14:30")];
    let names = |ns: [&str; 6]| ns.map(|n| format!("{n}.jpg")).to_vec();
    assert_eq!(chrono_order(&cat), names(["b1", "b2", "b3", "a1", "a2", "a3"]));

    // a1 and b1 are one moment: 10:00:00 − 08:59:30.
    let report = adjust(&mut cat, &b, &shift(3630));
    assert_eq!(report.adjusted, 3);

    // B is now 10:00:00, 10:05:00, 10:15:00 — interleaved with A.
    assert_eq!(chrono_order(&cat), names(["a1", "b1", "b2", "a2", "b3", "a3"]));
}

#[test]
fn adjusted_time_survives_a_rebuild_from_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg", Some("2024-06-01T12:00:00"), 0.0);
    adjust(&mut cat, &[id], &shift(-3600));

    // Derived cache lost; the sidecar is the source of truth.
    cat.conn().execute("UPDATE files SET taken_at = NULL", []).unwrap();
    cat.rebuild_from_sidecars(true).unwrap();

    assert_eq!(taken_at(&cat, id).as_deref(), Some("2024-06-01T11:00:00"));
}

#[test]
fn a_sidecar_without_a_date_never_erases_the_cached_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg", Some("2024-06-01T12:00:00"), 0.0);
    cat.set_rating(id, 5).unwrap();

    assert_eq!(taken_at(&cat, id).as_deref(), Some("2024-06-01T12:00:00"));
}
