//! Options-dialog settings persistence (TASK-01-022, PLAN §2 General tab).
//!
//! A small JSON file under the app data dir — the smallest thing that
//! satisfies the task's "Tauri store or a small settings table" spec and the
//! only store that survives catalog rebuilds (the catalog is a rebuildable
//! cache, PLAN §3 rule 2; settings are user data and must not live there).
//!
//! Writes follow the repo's atomic-replace discipline (`.tmp` + rename, same
//! as the sidecar writer): a crash mid-save can never truncate the file. A
//! missing or corrupt file falls back to defaults — never an error dialog.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Automatic-update policy (PLAN §2: Auto / check but ask / no).
///
/// Semantics (TASK-01-022 functional spec):
/// - [`UpdateMode::Auto`] checks and installs without asking.
/// - [`UpdateMode::CheckButAsk`] checks but prompts before installing.
/// - [`UpdateMode::Off`] never phones home (§1: no telemetry beyond update
///   checks — and with Off there isn't even that).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateMode {
    #[default]
    CheckButAsk,
    Auto,
    Off,
}

impl UpdateMode {
    /// Does this mode contact the update feed at all?
    pub fn phones_home(self) -> bool {
        !matches!(self, UpdateMode::Off)
    }

    /// Does this mode install without asking the user first?
    pub fn auto_installs(self) -> bool {
        matches!(self, UpdateMode::Auto)
    }
}

/// The General-tab settings (PLAN §2 Options dialog — General tab).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// UI language (BCP-47 tag). The i18n scaffold ships `en`; more locales
    /// plug in without a format change.
    pub language: String,
    pub update_mode: UpdateMode,
    /// Enable/disable hover tooltips.
    pub show_tooltips: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { language: "en".into(), update_mode: UpdateMode::CheckButAsk, show_tooltips: true }
    }
}

impl Settings {
    /// Load settings from `dir/settings.json`; missing or corrupt file →
    /// defaults. Never fails: options must not block app startup.
    pub fn load(dir: &Path) -> Self {
        match std::fs::read(dir.join("settings.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Atomically persist to `dir/settings.json` (`.tmp` + rename).
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_vec_pretty(self).expect("settings serialize infallibly");
        let tmp = dir.join("settings.json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, dir.join("settings.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let s = Settings::default();
        assert_eq!(s.language, "en");
        assert_eq!(s.update_mode, UpdateMode::CheckButAsk);
        assert!(s.show_tooltips);
    }

    #[test]
    fn update_mode_serializes_as_labeled() {
        // The labels the UI shows must match the persisted values.
        assert_eq!(serde_json::to_string(&UpdateMode::Auto).unwrap(), "\"auto\"");
        assert_eq!(serde_json::to_string(&UpdateMode::CheckButAsk).unwrap(), "\"check-but-ask\"");
        assert_eq!(serde_json::to_string(&UpdateMode::Off).unwrap(), "\"off\"");
    }
}
