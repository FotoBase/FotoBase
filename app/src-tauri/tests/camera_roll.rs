//! TASK-09-006: camera roll as a watched-folder equivalent.
//!
//! A platform change notification (MediaStore `ContentObserver`, iOS
//! `PHPhotoLibraryChangeObserver`) names the folders that hold camera roll
//! photos. It must become exactly the scan requests the folder watcher
//! produces for filesystem events — no parallel scanner — and new photos must
//! reach the catalog without a relaunch. The native observers are only
//! verifiable on a device or emulator; here they are the `Change` input.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fotobase_catalog::Catalog;
use fotobase_lib::camera_roll::{plan_scans, ScanRequest};
use fotobase_lib::folder_manager::FolderManager;

const DEBOUNCE: Duration = Duration::from_millis(300);
const SETTLE: Duration = Duration::from_millis(1500);

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

#[test]
fn change_for_a_watched_folder_is_a_rescan_of_that_folder() {
    let watched = [p("/sdcard/DCIM/Camera")];
    let plan = plan_scans(&watched, &[p("/sdcard/DCIM/Camera")]);
    assert_eq!(plan, vec![ScanRequest::Rescan(p("/sdcard/DCIM/Camera"))]);
}

#[test]
fn change_in_a_subfolder_rescans_the_watched_root_that_covers_it() {
    let watched = [p("/sdcard/DCIM")];
    let plan = plan_scans(&watched, &[p("/sdcard/DCIM/Camera"), p("/sdcard/DCIM/Burst")]);
    assert_eq!(plan, vec![ScanRequest::Rescan(p("/sdcard/DCIM"))], "one request per root");
}

#[test]
fn unknown_folder_becomes_a_watched_root() {
    let watched = [p("/sdcard/DCIM/Camera")];
    let plan = plan_scans(&watched, &[p("/sdcard/Pictures/Screenshots")]);
    assert_eq!(plan, vec![ScanRequest::Watch(p("/sdcard/Pictures/Screenshots"))]);
}

#[test]
fn new_folders_never_nest_and_repeats_collapse() {
    let plan = plan_scans(
        &[],
        &[p("/r/Pictures"), p("/r/Pictures/Shots"), p("/r/Pictures"), p("/r/DCIM/Camera")],
    );
    assert_eq!(
        plan,
        vec![ScanRequest::Watch(p("/r/Pictures")), ScanRequest::Watch(p("/r/DCIM/Camera"))]
    );
}

#[test]
fn empty_change_plans_nothing() {
    assert!(plan_scans(&[p("/a")], &[]).is_empty());
}

fn write_jpg(path: &Path, len: usize) {
    fs::write(path, vec![0xff; len]).unwrap();
}

fn count_files(catalog_path: &Path) -> i64 {
    let cat = Catalog::open(catalog_path).unwrap();
    cat.conn().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap()
}

fn wait_for(mut cond: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    cond()
}

fn normalize(path: &Path) -> PathBuf {
    path.components().collect()
}

/// Photos appear without manual import, and later ones without a relaunch.
#[test]
fn camera_roll_photos_reach_the_catalog_and_new_ones_follow() {
    let roll = tempfile::tempdir().unwrap();
    write_jpg(&roll.path().join("IMG_1.jpg"), 10);
    let db = tempfile::tempdir().unwrap();
    let catalog_path = db.path().join("catalog.db");
    drop(Catalog::open(&catalog_path).unwrap());
    let fm = FolderManager::open(&catalog_path, DEBOUNCE).unwrap();
    let folder = normalize(roll.path());

    // First notification: the folder is unknown, so it is watched and scanned.
    fm.camera_roll_changed(std::slice::from_ref(&folder)).unwrap();
    assert!(wait_for(|| count_files(&catalog_path) == 1, SETTLE), "existing photo not cataloged");

    // Watcher held back: only the platform notification can surface the photo.
    fm.pause_watching();
    write_jpg(&roll.path().join("IMG_2.jpg"), 20);
    fm.camera_roll_changed(std::slice::from_ref(&folder)).unwrap();
    fm.resume_watching();
    assert!(wait_for(|| count_files(&catalog_path) == 2, SETTLE), "new photo not picked up");

    // The folder stays a watched root, so a relaunch resumes it.
    fm.stop();
    let fm = FolderManager::open(&catalog_path, DEBOUNCE).unwrap();
    assert!(fm.has_watched_folders());
    fm.stop();
}
