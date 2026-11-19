//! Backup Manager, app side (TASK-08-014, PLAN §2L "Backup Pictures",
//! §6 P4): the persisted destination + schedule, the settings files that
//! ride along, and the job body that drives the catalog crate's engine.
//!
//! The engine ([`fotobase_catalog::backup`]) owns the set format and the
//! unplug-safety rules; this module only decides *what* is backed up and
//! *when*. Originals are never touched.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fotobase_catalog::backup::{
    library_sources, run_backup, BackupHooks, BackupProgress, BackupReport, BackupSchedule,
    BackupSource, EntryKind,
};
use fotobase_catalog::Catalog;
use serde::{Deserialize, Serialize};

/// Tauri event a run streams [`BackupProgress`] on (counters only, §3).
pub const BACKUP_EVENT: &str = "backup-progress";
/// Tauri event a *scheduled* run reports its outcome on (no caller awaits it).
pub const BACKUP_FINISHED_EVENT: &str = "backup-finished";
/// The Backup Manager's own settings file under the app data dir.
pub const CONFIG_FILE: &str = "backup.json";
/// App settings files backed up from the data dir when present.
pub const SETTINGS_FILES: [&str; 4] =
    ["settings.json", "catalogs.json", "associations.json", CONFIG_FILE];
/// How often the scheduler checks whether a backup is due.
pub const SCHEDULE_CHECK_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// Per-file pause while on battery: hashing is throttled, not stopped (§10).
pub const BATTERY_PACE: Duration = Duration::from_millis(25);

/// Destination and schedule chosen in the Backup Manager.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupConfig {
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub schedule: BackupSchedule,
}

impl BackupConfig {
    /// Missing or corrupt file → defaults (never blocks startup).
    pub fn load(data_dir: &Path) -> Self {
        std::fs::read(data_dir.join(CONFIG_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Atomic persist (`.tmp` + rename, like settings.json).
    pub fn save(&self, data_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(data_dir)?;
        let json = serde_json::to_vec_pretty(self).expect("backup config serializes infallibly");
        let tmp = data_dir.join(format!("{CONFIG_FILE}.tmp"));
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, data_dir.join(CONFIG_FILE))
    }

    /// The destination a scheduled run should write to now, if any: a
    /// schedule is on, the destination is plugged in, and the newest set
    /// there is older than the interval.
    pub fn due_destination(&self, now_unix: i64) -> Option<PathBuf> {
        let dest = PathBuf::from(self.destination.as_deref()?);
        if self.schedule == BackupSchedule::Off || !dest.is_dir() {
            return None;
        }
        let last = fotobase_catalog::backup::last_backup_unix(&dest);
        self.schedule.is_due(last, now_unix).then_some(dest)
    }
}

/// The app settings files under `data_dir` that exist.
pub fn settings_sources(data_dir: &Path) -> Vec<BackupSource> {
    SETTINGS_FILES
        .iter()
        .map(|name| data_dir.join(name))
        .filter(|p| p.is_file())
        .map(|path| BackupSource { kind: EntryKind::Settings, path })
        .collect()
}

/// One backup run on its own catalog connection: catalog snapshot, every
/// sidecar the catalog knows, `albums.json`, and the app settings files.
pub fn run(
    catalog_path: &Path,
    data_dir: &Path,
    dest: &Path,
    cancelled: &dyn Fn() -> bool,
    on_battery: &dyn Fn() -> bool,
    progress: &dyn Fn(&BackupProgress),
) -> Result<BackupReport, String> {
    let cat = Catalog::open(catalog_path).map_err(|e| e.to_string())?;
    let mut sources = library_sources(&cat).map_err(|e| e.to_string())?;
    sources.extend(settings_sources(data_dir));
    let pace = || {
        if on_battery() {
            std::thread::sleep(BATTERY_PACE);
        }
    };
    let hooks = BackupHooks { progress, cancelled, pace: &pace };
    run_backup(dest, Some(&cat), &sources, &hooks).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_and_defaults_off() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(BackupConfig::load(dir.path()), BackupConfig::default());
        let cfg = BackupConfig {
            destination: Some("/Volumes/Backup".into()),
            schedule: BackupSchedule::Weekly,
        };
        cfg.save(dir.path()).unwrap();
        assert_eq!(BackupConfig::load(dir.path()), cfg);
    }

    #[test]
    fn schedule_needs_a_plugged_in_destination() {
        let dest = tempfile::tempdir().unwrap();
        let mut cfg = BackupConfig {
            destination: Some(dest.path().to_string_lossy().into_owned()),
            schedule: BackupSchedule::Off,
        };
        assert_eq!(cfg.due_destination(0), None, "schedule off");
        cfg.schedule = BackupSchedule::Daily;
        assert_eq!(cfg.due_destination(0).as_deref(), Some(dest.path()), "no set yet");
        cfg.destination = Some(dest.path().join("unplugged").to_string_lossy().into_owned());
        assert_eq!(cfg.due_destination(0), None, "destination missing");
    }

    #[test]
    fn run_backs_up_catalog_sidecars_and_settings() {
        let data = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let catalog_path = data.path().join("catalog.db");
        drop(Catalog::open(&catalog_path).unwrap());
        std::fs::write(data.path().join("settings.json"), b"{}").unwrap();
        BackupConfig::default().save(data.path()).unwrap();

        let report =
            run(&catalog_path, data.path(), dest.path(), &|| false, &|| true, &|_| {}).unwrap();

        let m = fotobase_catalog::backup::read_manifest(Path::new(&report.set_path)).unwrap();
        let mut kinds: Vec<(EntryKind, String)> = m
            .entries
            .iter()
            .map(|e| {
                let name = Path::new(&e.path).file_name().unwrap().to_string_lossy().into_owned();
                (e.kind, name)
            })
            .collect();
        kinds.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(
            kinds,
            [
                (EntryKind::Settings, "backup.json".to_string()),
                (EntryKind::Catalog, "catalog.db".to_string()),
                (EntryKind::Settings, "settings.json".to_string()),
            ]
        );
    }
}
