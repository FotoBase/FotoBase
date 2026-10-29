//! System-tray icon for background watching (TASK-03-025, PLAN §6 P2a).
//!
//! The watcher lives in the Folder Manager backend, which outlives the main
//! window: closing the window hides it (configurable via the
//! `close_to_tray` setting, default on) and the tray keeps watching visible
//! and controllable. Quitting from the tray exits the process — the
//! watcher's threads die with it, so nothing is orphaned.
//!
//! All status shown in the tray is computed locally (PLAN §1: no telemetry,
//! no phone-home). The pure parts — menu-id → action mapping, status label,
//! pause toggle — are unit-tested here without a running app.

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Manager,
};

use crate::folder_manager::FolderManager;

/// Tray menu item ids (stable strings, matched in [`tray_action`]).
pub const MENU_SHOW: &str = "fb-tray-show";
pub const MENU_TOGGLE_PAUSE: &str = "fb-tray-toggle-pause";
pub const MENU_QUIT: &str = "fb-tray-quit";

/// What a tray menu item means. The unit-testable half of the mapping:
/// [`tray_action`] turns the menu id into the action, the builder's
/// `on_menu_event` runs it against the app state.
#[derive(Debug, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    TogglePause,
    Quit,
}

/// Map a tray menu-item id to its action; unknown ids are ignored (Tauri
/// also synthesizes separator/quit ids we must not match by accident).
pub fn tray_action(id: &str) -> Option<TrayAction> {
    match id {
        MENU_SHOW => Some(TrayAction::Show),
        MENU_TOGGLE_PAUSE => Some(TrayAction::TogglePause),
        MENU_QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

/// Tray status: what the watcher is doing right now, shown in the tooltip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchStatus {
    /// No folder is watched (`scan_mode = 'always'`).
    Idle,
    /// At least one folder watched, nothing happening.
    Watching,
    /// A watched-folder scan is running.
    Scanning,
    /// Watching is paused from the tray.
    Paused,
}

impl WatchStatus {
    pub fn label(self) -> &'static str {
        match self {
            WatchStatus::Idle => "idle",
            WatchStatus::Watching => "watching",
            WatchStatus::Scanning => "scanning",
            WatchStatus::Paused => "paused",
        }
    }

    /// Compute the tray status from the Folder Manager's live state.
    /// `has_watched` is passed in (not queried) so the precedence rules are
    /// testable without a catalog: paused > scanning > watching > idle.
    pub fn from_state(paused: bool, scanning: bool, has_watched: bool) -> Self {
        if paused {
            WatchStatus::Paused
        } else if scanning {
            WatchStatus::Scanning
        } else if has_watched {
            WatchStatus::Watching
        } else {
            WatchStatus::Idle
        }
    }
}

/// The watcher controls the tray menu drives. [`FolderManager`] is the real
/// implementation; the unit tests use a fake.
pub trait WatchControls {
    fn pause(&self);
    fn resume(&self);
    fn is_paused(&self) -> bool;
    fn is_scanning(&self) -> bool;
    fn has_watched_folders(&self) -> bool;
}

impl WatchControls for FolderManager {
    fn pause(&self) {
        self.pause_watching();
    }
    fn resume(&self) {
        self.resume_watching();
    }
    fn is_paused(&self) -> bool {
        FolderManager::is_paused(self)
    }
    fn is_scanning(&self) -> bool {
        FolderManager::is_scanning(self)
    }
    fn has_watched_folders(&self) -> bool {
        FolderManager::has_watched_folders(self)
    }
}

/// Toggle watching from the tray; returns the new paused state so the
/// caller can update the menu label without re-reading the manager.
pub fn toggle_pause(controls: &impl WatchControls) -> bool {
    if controls.is_paused() {
        controls.resume();
    } else {
        controls.pause();
    }
    controls.is_paused()
}

/// Tooltip text for the current status (no dynamic data, no telemetry).
pub fn tooltip(status: WatchStatus) -> String {
    format!("Fotobase — {status}", status = status.label())
}

/// Build the tray icon + menu and wire the menu events. The menu:
/// "Show Fotobase", a separator, "Pause Watching"/"Resume Watching"
/// (updated in place as the state flips), another separator, "Quit".
pub fn build_tray(app: &AppHandle) -> Result<TrayIcon, tauri::Error> {
    let show = MenuItem::with_id(app, MENU_SHOW, "Show Fotobase", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, MENU_TOGGLE_PAUSE, "Pause Watching", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &sep1, &toggle, &sep2, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip(tooltip(WatchStatus::Idle))
        .on_menu_event(|app, event| match tray_action(event.id().as_ref()) {
            Some(TrayAction::Show) => show_main_window(app),
            Some(TrayAction::TogglePause) => {
                let now_paused = with_controls(app, toggle_pause).unwrap_or(false);
                if let Some(ui) = app.try_state::<TrayUi>() {
                    let status = with_controls(app, current_status).unwrap_or(WatchStatus::Idle);
                    update_tray_labels(&ui, now_paused, &status);
                }
            }
            // Quit fully exits: see [`quit`].
            Some(TrayAction::Quit) => quit(app),
            None => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let tray = builder.build(app)?;
    // The updater and the menu handler reach the icon and the pause/resume
    // item through this managed handle (TrayIcon has no menu getter).
    app.manage(TrayUi { tray: tray.clone(), toggle });
    Ok(tray)
}

/// Managed handle to the tray pieces the status updater and the menu
/// handler mutate: the icon (tooltip) and the pause/resume menu item (text).
pub struct TrayUi {
    tray: TrayIcon<tauri::Wry>,
    toggle: MenuItem<tauri::Wry>,
}

impl TrayUi {
    fn set_tooltip(&self, text: String) {
        let _ = self.tray.set_tooltip(Some(text));
    }

    fn set_toggle_label(&self, paused: bool) {
        let _ = self.toggle.set_text(if paused { "Resume Watching" } else { "Pause Watching" });
    }
}

/// Stable tray id so menu handlers can look the icon up to update its
/// tooltip/menu label in place.
pub const TRAY_ID: &str = "fotobase-tray";

/// Quit fully exits: the watcher threads are process-owned, so exiting
/// ends watching — no orphan processes (spec). Shared by the tray menu
/// item and the `quit_from_tray` command (the e2e suite reaches this exact
/// code path because WebDriver cannot click the OS tray).
pub fn quit(app: &AppHandle) {
    app.exit(0);
}

fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// Run `f` against the Folder Manager when one exists (it is created lazily
/// on the first folder-manager command; before that there is nothing to
/// watch, and the tray still works — Show/Quit).
fn with_controls<T>(app: &AppHandle, f: impl FnOnce(&FolderManager) -> T) -> Option<T> {
    let state = app.try_state::<crate::AppState>()?;
    let guard = state.folders.lock().ok()?;
    let fm = guard.as_ref()?;
    Some(f(fm))
}

/// Current status from the live manager; idle when none exists yet.
fn current_status(fm: &FolderManager) -> WatchStatus {
    WatchStatus::from_state(fm.is_paused(), fm.is_scanning(), fm.has_watched_folders())
}

/// Reflect the state in the tooltip and the pause/resume item label.
fn update_tray_labels(ui: &TrayUi, paused: bool, status: &WatchStatus) {
    ui.set_tooltip(if paused { tooltip(WatchStatus::Paused) } else { tooltip(*status) });
    ui.set_toggle_label(paused);
}

/// Poll the manager's live state and refresh the tray labels when it
/// changes (idle / watching / scanning): a scan starting or finishing
/// shows up here without any IPC from the frontend, and everything this
/// reads is local (PLAN §1: no telemetry). Cheap: one mutex peek + two
/// atomics per tick, and a change only rewrites the tooltip.
pub fn spawn_status_updater(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last: Option<(bool, WatchStatus)> = None;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let Some(state) = app.try_state::<crate::AppState>() else { continue };
            let (paused, status) = {
                let guard = match state.folders.lock() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                match guard.as_ref() {
                    Some(fm) => (fm.is_paused(), current_status(fm)),
                    // No Folder Manager yet: nothing is watched.
                    None => (false, WatchStatus::Idle),
                }
            };
            if last == Some((paused, status)) {
                continue;
            }
            last = Some((paused, status));
            if let Some(ui) = app.try_state::<TrayUi>() {
                update_tray_labels(&ui, paused, &status);
            }
        }
    });
}

/// Window-close interception (TASK-03-025): with the `close_to_tray`
/// setting on, the close button hides the window instead of quitting so
/// watching continues from the tray; off, the app exits as before.
pub fn handle_close_request(app: &AppHandle) -> bool {
    let close_to_tray = app
        .try_state::<crate::AppState>()
        .and_then(|state| state.settings.lock().ok().map(|s| s.close_to_tray))
        .unwrap_or(true);
    if !close_to_tray {
        return false;
    }
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_to_actions() {
        assert_eq!(tray_action(MENU_SHOW), Some(TrayAction::Show));
        assert_eq!(tray_action(MENU_TOGGLE_PAUSE), Some(TrayAction::TogglePause));
        assert_eq!(tray_action(MENU_QUIT), Some(TrayAction::Quit));
        // Tauri's predefined items and anything unknown must be ignored.
        assert_eq!(tray_action("separator"), None);
        assert_eq!(tray_action(""), None);
    }

    #[test]
    fn status_precedence_paused_over_scanning_over_watching() {
        assert_eq!(WatchStatus::from_state(true, true, true), WatchStatus::Paused);
        assert_eq!(WatchStatus::from_state(false, true, true), WatchStatus::Scanning);
        assert_eq!(WatchStatus::from_state(false, false, true), WatchStatus::Watching);
        assert_eq!(WatchStatus::from_state(false, false, false), WatchStatus::Idle);
    }

    #[test]
    fn status_labels_match_spec_vocabulary() {
        assert_eq!(WatchStatus::Idle.label(), "idle");
        assert_eq!(WatchStatus::Watching.label(), "watching");
        assert_eq!(WatchStatus::Scanning.label(), "scanning");
        assert_eq!(WatchStatus::Paused.label(), "paused");
    }

    struct FakeControls {
        paused: std::cell::Cell<bool>,
    }

    impl WatchControls for FakeControls {
        fn pause(&self) {
            self.paused.set(true);
        }
        fn resume(&self) {
            self.paused.set(false);
        }
        fn is_paused(&self) -> bool {
            self.paused.get()
        }
        fn is_scanning(&self) -> bool {
            false
        }
        fn has_watched_folders(&self) -> bool {
            true
        }
    }

    #[test]
    fn toggle_pause_flips_watcher_state() {
        let fake = FakeControls { paused: std::cell::Cell::new(false) };
        // `toggle_pause` returns the new paused state after the flip.
        assert!(toggle_pause(&fake), "first toggle pauses");
        assert!(fake.is_paused());
        assert!(!toggle_pause(&fake), "second toggle resumes");
        assert!(!fake.is_paused());
    }

    #[test]
    fn tooltip_mentions_app_and_status() {
        assert_eq!(tooltip(WatchStatus::Watching), "Fotobase — watching");
    }
}
