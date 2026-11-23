//! TASK-08-019 integration: a (simulated) PrtScn press lands a timestamped
//! PNG in `<catalog>/Screen Captures/`, which is a Scan Always folder — the
//! watcher puts it in the catalog without any manual scan. Opted out, the
//! press does nothing at all.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fotobase_catalog::{Catalog, ScanMode};
use fotobase_lib::folder_manager::FolderManager;
use fotobase_lib::screen_capture::{
    capture_and_watch, CaptureDisplay, CaptureError, CaptureOutcome, ScreenGrabber,
};
use fotobase_lib::settings::Settings;

const DEBOUNCE: Duration = Duration::from_millis(300);
const SETTLE: Duration = Duration::from_millis(3000);

struct OnePng;

impl ScreenGrabber for OnePng {
    fn grab(&self, _: CaptureDisplay, out_dir: &Path) -> Result<Vec<PathBuf>, CaptureError> {
        let path = out_dir.join("screen.png");
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n-pixels-")?;
        Ok(vec![path])
    }
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

fn file_names(catalog_path: &Path) -> Vec<String> {
    let cat = Catalog::open(catalog_path).unwrap();
    cat.list_files(100).unwrap().into_iter().map(|f| f.filename).collect()
}

fn now() -> jiff::ZonedDateTime {
    "2026-11-23T17:13:47+01:00[+01:00]".parse().unwrap()
}

#[test]
fn a_press_saves_a_png_the_watcher_lists_in_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let catalog_path = dir.path().join("catalog.db");
    drop(Catalog::open(&catalog_path).unwrap());
    let fm = FolderManager::open(&catalog_path, DEBOUNCE).unwrap();
    let settings = Settings { screen_capture_enabled: true, ..Settings::default() };

    let outcome = capture_and_watch(&settings, dir.path(), &fm, &OnePng, &now()).unwrap();

    let png = dir.path().join("Screen Captures").join("Screen Capture 2026-11-23 17.13.47.png");
    assert_eq!(outcome, CaptureOutcome::Saved(vec![png.clone()]));
    assert!(png.is_file());
    assert!(
        wait_for(
            || file_names(&catalog_path).contains(&"Screen Capture 2026-11-23 17.13.47.png".into()),
            SETTLE
        ),
        "watcher never listed the capture"
    );
    let cat = Catalog::open(&catalog_path).unwrap();
    let folder =
        cat.list_folders().unwrap().into_iter().find(|f| f.path.ends_with("Screen Captures"));
    assert_eq!(folder.expect("Screen Captures folder registered").scan_mode, ScanMode::Always);
    fm.stop();
}

#[test]
fn without_opt_in_nothing_is_captured_or_registered() {
    let dir = tempfile::tempdir().unwrap();
    let catalog_path = dir.path().join("catalog.db");
    drop(Catalog::open(&catalog_path).unwrap());
    let fm = FolderManager::open(&catalog_path, DEBOUNCE).unwrap();

    let outcome =
        capture_and_watch(&Settings::default(), dir.path(), &fm, &OnePng, &now()).unwrap();

    assert_eq!(outcome, CaptureOutcome::Disabled);
    assert!(!dir.path().join("Screen Captures").exists());
    assert!(Catalog::open(&catalog_path).unwrap().list_folders().unwrap().is_empty());
    fm.stop();
}
