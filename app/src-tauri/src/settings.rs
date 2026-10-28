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

/// Library grid renderer (TASK-01-014, PLAN §6 P1). Canvas is a planned
/// deliverable, not a contingency — but no accepted ADR has chosen it as an
/// OS default yet, so [`GridRenderer::Dom`] stays the default everywhere and
/// canvas is an explicit user override available on every OS.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GridRenderer {
    #[default]
    Dom,
    Canvas,
}

/// UI theme (TASK-01-024, PLAN §2 additions "Dark mode / HiDPI").
///
/// `System` follows the OS setting (the default); `Light`/`Dark` are the
/// explicit app toggle. The frontend resolves this to a `data-theme`
/// attribute the stylesheet keys off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// What the grid prints under each thumbnail (TASK-03-015, PLAN §6 P2a
/// "Thumbnail captions"). Display-only: resolved from the grid row, never
/// written back to a sidecar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptionMode {
    #[default]
    None,
    Filename,
    Caption,
    Tags,
    Resolution,
}

/// Key the Folders view sorts by (TASK-03-016, PLAN §6 P2a). Name is A→Z,
/// date is newest first, size is largest first; the reverse flag flips any.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FolderSort {
    #[default]
    Name,
    Date,
    Size,
}

/// Folders-view layout (TASK-03-016): the P1 year-grouped tree, or one flat
/// sortable list of every folder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FolderView {
    #[default]
    Tree,
    Flat,
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
    /// Which renderer draws the library grid (TASK-01-014).
    #[serde(default)]
    pub grid_renderer: GridRenderer,
    /// Dark mode: follow the OS, or force light/dark (TASK-01-024).
    #[serde(default)]
    pub theme: Theme,
    /// Image types the user registered Fotobase as the OS default viewer
    /// for (TASK-01-025, PLAN §2M). Lowercase, dotless; the list is the
    /// Options dialog's source of truth, the OS registration is applied
    /// through the `associations` module when it changes.
    #[serde(default)]
    pub file_associations: Vec<String>,
    /// Opt-in 1–5 ratings (TASK-03-005, ADR-0002). Off: single star, which
    /// maps to `xmp:Rating` = 5.
    #[serde(default)]
    pub ratings_enabled: bool,
    /// Quick Tags panel labels (TASK-03-006, PLAN §2B): at most 10
    /// user-configurable one-click tags. Empty slots render disabled.
    #[serde(default)]
    pub quick_tags: Vec<String>,
    /// Reveal hidden photos/folders, grayed (TASK-03-009).
    #[serde(default)]
    pub show_hidden: bool,
    /// Catalog-trash undo window in days (TASK-03-012); older entries are
    /// swept on startup.
    #[serde(default = "default_trash_window_days")]
    pub trash_window_days: u32,
    /// Thumbnail caption mode (TASK-03-015).
    #[serde(default)]
    pub caption_mode: CaptionMode,
    /// Folders-view sort key, reverse flag, and layout (TASK-03-016).
    #[serde(default)]
    pub folder_sort: FolderSort,
    #[serde(default)]
    pub folder_sort_reverse: bool,
    #[serde(default)]
    pub folder_view: FolderView,
    /// A single click opens the edit view instead of selecting (TASK-03-017,
    /// PLAN §6 P2a). Off by default.
    #[serde(default)]
    pub single_click_edit: bool,
}

fn default_trash_window_days() -> u32 {
    fotobase_catalog::DEFAULT_TRASH_WINDOW_DAYS
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "en".into(),
            update_mode: UpdateMode::CheckButAsk,
            show_tooltips: true,
            grid_renderer: GridRenderer::default(),
            theme: Theme::default(),
            file_associations: Vec::new(),
            ratings_enabled: false,
            quick_tags: Vec::new(),
            show_hidden: false,
            trash_window_days: default_trash_window_days(),
            caption_mode: CaptionMode::default(),
            folder_sort: FolderSort::default(),
            folder_sort_reverse: false,
            folder_view: FolderView::default(),
            single_click_edit: false,
        }
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
        assert_eq!(s.grid_renderer, GridRenderer::Dom);
    }

    #[test]
    fn theme_defaults_to_system_and_serializes_kebab() {
        // TASK-01-024: dark mode follows the OS by default; the app toggle
        // is an explicit override.
        assert_eq!(Settings::default().theme, Theme::System);
        assert_eq!(serde_json::to_string(&Theme::System).unwrap(), "\"system\"");
        assert_eq!(serde_json::to_string(&Theme::Light).unwrap(), "\"light\"");
        assert_eq!(serde_json::to_string(&Theme::Dark).unwrap(), "\"dark\"");
    }

    #[test]
    fn older_settings_files_without_a_theme_still_load() {
        // Pre-01-024 settings.json has no theme field; loading it must not
        // reset the user's other settings to defaults.
        let dir = std::env::temp_dir().join(format!("fb-settings-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let json = br#"{"language":"de","update_mode":"off","show_tooltips":false,"grid_renderer":"canvas"}"#;
        std::fs::write(dir.join("settings.json"), json).unwrap();
        let s = Settings::load(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(s.language, "de");
        assert_eq!(s.update_mode, UpdateMode::Off);
        assert!(!s.show_tooltips);
        assert_eq!(s.grid_renderer, GridRenderer::Canvas);
        assert_eq!(s.theme, Theme::System);
    }

    #[test]
    fn update_mode_serializes_as_labeled() {
        // The labels the UI shows must match the persisted values.
        assert_eq!(serde_json::to_string(&UpdateMode::Auto).unwrap(), "\"auto\"");
        assert_eq!(serde_json::to_string(&UpdateMode::CheckButAsk).unwrap(), "\"check-but-ask\"");
        assert_eq!(serde_json::to_string(&UpdateMode::Off).unwrap(), "\"off\"");
    }

    // TASK-01-014: the grid renderer is a user-overridable config knob. The
    // default stays DOM — no accepted ADR has chosen canvas as an OS default.

    #[test]
    fn grid_renderer_defaults_to_dom() {
        let s = Settings::default();
        assert_eq!(s.grid_renderer, GridRenderer::Dom);
    }

    #[test]
    fn grid_renderer_serializes_as_labeled() {
        assert_eq!(serde_json::to_string(&GridRenderer::Dom).unwrap(), "\"dom\"");
        assert_eq!(serde_json::to_string(&GridRenderer::Canvas).unwrap(), "\"canvas\"");
    }

    #[test]
    fn settings_written_before_the_field_still_load() {
        // A settings.json from before TASK-01-014 has no `grid_renderer` key.
        // It must parse (field defaults) — NOT fall back to all-defaults,
        // which would silently lose the user's language and update mode.
        let old = r#"{"language":"de","update_mode":"off","show_tooltips":false}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(s.language, "de");
        assert_eq!(s.update_mode, UpdateMode::Off);
        assert!(!s.show_tooltips);
        assert_eq!(s.grid_renderer, GridRenderer::Dom);
    }

    #[test]
    fn ratings_are_opt_in_and_survive_old_settings_files() {
        // ADR-0002: single star by default; a pre-03-005 settings.json has no
        // `ratings_enabled` key and must keep the user's other settings.
        assert!(!Settings::default().ratings_enabled);
        let old = r#"{"language":"de","update_mode":"off","show_tooltips":false}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(s.language, "de");
        assert!(!s.ratings_enabled);
    }

    #[test]
    fn file_associations_default_to_empty_and_survive_old_files() {
        // TASK-01-025: the association list is user state; a settings.json
        // from before the field existed must not reset the other settings.
        assert!(Settings::default().file_associations.is_empty());
        let old = r#"{"language":"de","update_mode":"off","show_tooltips":false}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(s.language, "de");
        assert!(s.file_associations.is_empty());
    }
}
