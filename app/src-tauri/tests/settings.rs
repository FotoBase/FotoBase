//! TASK-01-022 unit tests: every setting round-trips through persistence.
//!
//! Settings live in a small JSON file under the app data dir (TASK-01-022
//! technical spec: "Tauri store or a small settings table" — a file is the
//! smallest thing that satisfies the spec and survives catalog rebuilds,
//! since the catalog is a rebuildable cache per PLAN §3 rule 2).

use fotobase_lib::settings::{Settings, UpdateMode};
use tempfile::TempDir;

fn write_settings(dir: &TempDir, json: &str) {
    std::fs::write(dir.path().join("settings.json"), json).expect("write settings fixture");
}

#[test]
fn defaults_when_no_file_exists() {
    let dir = TempDir::new().expect("tempdir");
    let s = Settings::load(dir.path());
    assert_eq!(s.language, "en");
    assert_eq!(s.update_mode, UpdateMode::CheckButAsk);
    assert!(s.show_tooltips);
}

#[test]
fn corrupt_file_falls_back_to_defaults() {
    let dir = TempDir::new().expect("tempdir");
    write_settings(&dir, "{ not json");
    let s = Settings::load(dir.path());
    assert_eq!(s, Settings::default());
}

#[test]
fn language_round_trips() {
    let dir = TempDir::new().expect("tempdir");
    let s = Settings { language: "de".into(), ..Settings::default() };
    s.save(dir.path()).expect("save");
    assert_eq!(Settings::load(dir.path()).language, "de");
}

#[test]
fn update_mode_round_trips_all_variants() {
    for mode in [UpdateMode::Auto, UpdateMode::CheckButAsk, UpdateMode::Off] {
        let dir = TempDir::new().expect("tempdir");
        let s = Settings { update_mode: mode, ..Settings::default() };
        s.save(dir.path()).expect("save");
        assert_eq!(Settings::load(dir.path()).update_mode, mode);
    }
}

#[test]
fn show_tooltips_round_trips() {
    let dir = TempDir::new().expect("tempdir");
    let s = Settings { show_tooltips: false, ..Settings::default() };
    s.save(dir.path()).expect("save");
    assert!(!Settings::load(dir.path()).show_tooltips);
}

#[test]
fn save_is_atomic_no_tmp_left_behind() {
    let dir = TempDir::new().expect("tempdir");
    Settings::default().save(dir.path()).expect("save");
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .expect("read dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["settings.json".to_string()], "no .tmp leftovers");
}

#[test]
fn update_mode_phones_home_semantics() {
    // §1: no telemetry beyond update checks; "off" must never phone home.
    assert!(UpdateMode::Auto.phones_home());
    assert!(UpdateMode::CheckButAsk.phones_home());
    assert!(!UpdateMode::Off.phones_home());
    // Only Auto installs without asking.
    assert!(UpdateMode::Auto.auto_installs());
    assert!(!UpdateMode::CheckButAsk.auto_installs());
    assert!(!UpdateMode::Off.auto_installs());
}
