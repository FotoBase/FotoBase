//! Watcher pause/resume tests (TASK-03-025, TDD).
//!
//! The tray's "pause/resume watching" maps onto [`Watcher`]: while paused,
//! watched-folder events must NOT trigger scans (they queue instead), and
//! resuming must flush everything that queued — no event observed while
//! paused is lost.

use std::fs;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fotobase_catalog::Catalog;
use fotobase_scanner::{scan_folder, Watcher};

// Same rationale as tests/watcher.rs: the debounce window must dwarf CI
// scheduling stalls.
const DEBOUNCE: Duration = Duration::from_millis(300);
const SETTLE: Duration = Duration::from_millis(1200);

fn wait_for(mut cond: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    cond()
}

struct Fixture {
    _lib: tempfile::TempDir,
    _db: tempfile::TempDir,
    catalog: Arc<Mutex<Catalog>>,
    handle: Watcher,
}

fn fixture() -> Fixture {
    let lib = tempfile::tempdir().unwrap();
    fs::write(lib.path().join("seed.jpg"), vec![0xff; 512]).unwrap();
    let db = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(db.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, lib.path(), &AtomicBool::new(false)).unwrap();
    let catalog = Arc::new(Mutex::new(cat));
    let handle = Watcher::spawn(&[lib.path().to_path_buf()], catalog.clone(), DEBOUNCE).unwrap();
    Fixture { _lib: lib, _db: db, catalog, handle }
}

impl Fixture {
    fn write(&self, name: &str) {
        fs::write(self._lib.path().join(name), vec![0xee; 256]).unwrap();
    }

    fn count(&self) -> i64 {
        self.catalog
            .lock()
            .unwrap()
            .conn()
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
            .unwrap()
    }
}

#[test]
fn pause_gates_scans_until_resume() {
    let fx = fixture();
    let before = fx.handle.scan_count();

    fx.handle.pause();
    assert!(fx.handle.is_paused());

    fx.write("paused-drop.jpg");
    // Well past the debounce window: nothing may have been scanned.
    assert!(!wait_for(|| fx.handle.scan_count() > before, SETTLE));
    assert_eq!(fx.count(), 1, "no file may enter the catalog while paused");

    fx.handle.resume();
    assert!(!fx.handle.is_paused());
    // The event observed while paused is queued, not dropped: resuming
    // must scan it.
    assert!(wait_for(|| fx.handle.scan_count() > before, SETTLE));
    assert!(wait_for(|| fx.count() == 2, SETTLE));
}

#[test]
fn resume_without_pending_events_is_a_quiet_noop() {
    let fx = fixture();
    let before = fx.handle.scan_count();
    fx.handle.pause();
    fx.handle.resume();
    // No event ever fired: no catch-up scan may spin up.
    assert!(!wait_for(|| fx.handle.scan_count() > before, SETTLE));
}

#[test]
fn is_scanning_false_when_idle() {
    let fx = fixture();
    std::thread::sleep(DEBOUNCE + Duration::from_millis(200));
    assert!(!fx.handle.is_scanning());
}
