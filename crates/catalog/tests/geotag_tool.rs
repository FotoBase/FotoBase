//! TASK-06-006: geotag tool. Tools → Geotag applies one location to a
//! single photo, a selection, or a whole folder/album in one pass, with
//! undo. The GPS lands in the sidecar (source of truth, PLAN §5), the
//! catalog mirror follows, originals never change (PLAN §3 rule 1), and
//! undo restores the GPS values the photos had before the batch.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{sidecar_path, Catalog};
use fotobase_metadata::Sidecar;

fn add_photo(cat: &mut Catalog, dir: &Path, name: &str) -> (i64, PathBuf) {
    let original = dir.join(name);
    std::fs::write(&original, b"fake image bytes").unwrap();
    let folder_id = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, hash, media_type)
             VALUES (?1, ?2, ?3, 10, 0.0, ?4, 'image')",
            rusqlite::params![folder_id, original.to_str().unwrap(), name, format!("hash-{name}")],
        )
        .unwrap();
    (cat.conn().last_insert_rowid(), original)
}

fn sidecar_gps(path: &Path) -> Option<(f64, f64)> {
    Sidecar::read(&sidecar_path(path.to_str().unwrap())).ok().and_then(|sc| sc.gps())
}

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

fn progress(_: usize, _: usize) {}

#[test]
fn tool_geotags_a_whole_folder_in_one_pass() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (a, a_path) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (b, b_path) = add_photo(&mut cat, dir.path(), "b.jpg");
    let (c, c_path) = add_photo(&mut cat, dir.path(), "c.jpg");
    let before: Vec<Vec<u8>> =
        [&a_path, &b_path, &c_path].iter().map(|p| std::fs::read(p).unwrap()).collect();
    let folder_id = cat
        .conn()
        .query_row("SELECT folder_id FROM files WHERE id = ?1", [a], |r| r.get::<_, i64>(0))
        .unwrap();

    let report = cat
        .geotag_tool_run(
            &fotobase_catalog::GeotagTarget::Folder { folder_id },
            48.8566,
            2.3522,
            &no_cancel(),
            &progress,
        )
        .unwrap();

    assert_eq!(report.applied, 3, "{report:?}");
    assert!(report.failed.is_empty(), "{report:?}");
    assert!(!report.cancelled);
    for (id, path) in [(a, &a_path), (b, &b_path), (c, &c_path)] {
        let (lat, lon) = sidecar_gps(path).unwrap_or_else(|| panic!("id {id} has no GPS"));
        assert!((lat - 48.8566).abs() < 1e-6, "id {id}: {lat}");
        assert!((lon - 2.3522).abs() < 1e-6, "id {id}: {lon}");
    }
    for (path, was) in [(&a_path, &before[0]), (&b_path, &before[1]), (&c_path, &before[2])] {
        assert_eq!(&std::fs::read(path).unwrap(), was, "original untouched");
    }
    assert_eq!(cat.geotagged_pins().unwrap().len(), 3);
}

#[test]
fn tool_geotags_an_album_and_the_current_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (a, a_path) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (b, b_path) = add_photo(&mut cat, dir.path(), "b.jpg");
    let album_id = cat.create_album("Trip", "2026-01-01", None, None).unwrap().id;
    cat.add_to_album(album_id, &[a, b]).unwrap();

    let report = cat
        .geotag_tool_run(
            &fotobase_catalog::GeotagTarget::Album { album_id },
            41.9028,
            12.4964,
            &no_cancel(),
            &progress,
        )
        .unwrap();
    assert_eq!(report.applied, 2, "{report:?}");
    assert!(sidecar_gps(&a_path).is_some());
    assert!(sidecar_gps(&b_path).is_some());

    let report = cat
        .geotag_tool_run(
            &fotobase_catalog::GeotagTarget::Ids(vec![a]),
            0.0,
            0.0,
            &no_cancel(),
            &progress,
        )
        .unwrap();
    assert_eq!(report.applied, 1, "{report:?}");
    let (lat, lon) = sidecar_gps(&a_path).unwrap();
    assert!((lat - 0.0).abs() < 1e-9 && (lon - 0.0).abs() < 1e-9);
}

#[test]
fn undo_restores_the_prior_gps_values() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (a, a_path) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (b, b_path) = add_photo(&mut cat, dir.path(), "b.jpg");
    // a had a GPS value before the tool ran; b had none.
    cat.geotag_photos(&[a], 10.0, 20.0).unwrap();

    cat.geotag_tool_run(
        &fotobase_catalog::GeotagTarget::Ids(vec![a, b]),
        52.0,
        13.0,
        &no_cancel(),
        &progress,
    )
    .unwrap();
    assert_eq!(sidecar_gps(&a_path).unwrap().0, 52.0);

    cat.undo_geotag(&progress).unwrap();

    assert_eq!(sidecar_gps(&a_path).unwrap(), (10.0, 20.0), "a restored");
    assert!(sidecar_gps(&b_path).is_none(), "b back to untagged");
    assert_eq!(cat.geotagged_pins().unwrap().len(), 1, "mirror keeps a's restored GPS");
}

#[test]
fn undo_without_a_batch_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    assert_eq!(cat.undo_geotag(&progress).unwrap(), 0);
}

#[test]
fn cancelling_mid_batch_keeps_processed_photos_valid_and_stops_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let mut ids = Vec::new();
    for n in 0..6 {
        let (id, _) = add_photo(&mut cat, dir.path(), &format!("p{n}.jpg"));
        ids.push(id);
    }
    let cancel = AtomicBool::new(false);
    let seen = std::cell::Cell::new(0);
    let report = cat
        .geotag_tool_run(
            &fotobase_catalog::GeotagTarget::Ids(ids.clone()),
            1.0,
            2.0,
            &cancel,
            &|done, total| {
                seen.set(done);
                if done >= 2 {
                    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                let _ = total;
            },
        )
        .unwrap();

    assert!(report.cancelled, "{report:?}");
    assert!(report.applied >= 2 && report.applied < 6, "{report:?}");
    assert_eq!(seen.get(), report.applied);
    let mut tagged = 0;
    for &id in &ids {
        let path: String = cat
            .conn()
            .query_row("SELECT path FROM files WHERE id = ?1", [id], |r| r.get(0))
            .unwrap();
        if sidecar_gps(Path::new(&path)).is_some() {
            tagged += 1;
        }
    }
    assert_eq!(tagged, report.applied as usize, "exactly the processed photos tagged");
}

#[test]
fn a_new_batch_replaces_the_previous_undo_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (a, a_path) = add_photo(&mut cat, dir.path(), "a.jpg");

    cat.geotag_tool_run(
        &fotobase_catalog::GeotagTarget::Ids(vec![a]),
        1.0,
        1.0,
        &no_cancel(),
        &progress,
    )
    .unwrap();
    cat.geotag_tool_run(
        &fotobase_catalog::GeotagTarget::Ids(vec![a]),
        2.0,
        2.0,
        &no_cancel(),
        &progress,
    )
    .unwrap();
    cat.undo_geotag(&progress).unwrap();

    // One undo step: back to the state before the *latest* batch.
    assert_eq!(sidecar_gps(&a_path).unwrap(), (1.0, 1.0));
    // And the snapshot is spent — a second undo does nothing.
    assert_eq!(cat.undo_geotag(&progress).unwrap(), 0);
}

#[test]
fn out_of_range_coordinates_are_refused_with_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (a, a_path) = add_photo(&mut cat, dir.path(), "a.jpg");

    assert!(cat
        .geotag_tool_run(
            &fotobase_catalog::GeotagTarget::Ids(vec![a]),
            91.0,
            0.0,
            &no_cancel(),
            &progress
        )
        .is_err());
    assert!(cat
        .geotag_tool_run(
            &fotobase_catalog::GeotagTarget::Ids(vec![a]),
            0.0,
            180.5,
            &no_cancel(),
            &progress
        )
        .is_err());
    assert!(!sidecar_path(a_path.to_str().unwrap()).exists(), "no sidecar written");
}
