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
    AppHandle, Manager, Wry,
};

use crate::catalog_db_path;
use crate::folder_manager::FolderManager;

/// Tray menu item ids (stable strings, matched in [`tray_action`]).
pub const MENU_SHOW: &str = "fb-tray-show";
pub const MENU_TOGGLE_PAUSE: &str = "fb-tray-toggle-pause";
pub const MENU_QUIT: &str = "fb-tray-quit";
/// Prefix for per-catalog switch items (TASK-03-024): `fb-tray-catalog-<n>`
/// with n the index into the registry's catalog list *at event time*.
pub const MENU_CATALOG_PREFIX: &str = "fb-tray-catalog-";

/// What a tray menu item means. The unit-testable half of the mapping:
/// [`tray_action`] turns the menu id into the action, the builder's
/// `on_menu_event` runs it against the app state.
#[derive(Debug, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    TogglePause,
    Quit,
    /// Switch to the catalog at this registry index (TASK-03-024).
    SwitchCatalog(usize),
}

/// Map a tray menu-item id to its action; unknown ids are ignored (Tauri
/// also synthesizes separator/quit ids we must not match by accident).
pub fn tray_action(id: &str) -> Option<TrayAction> {
    if let Some(idx) = id.strip_prefix(MENU_CATALOG_PREFIX) {
        return idx.parse::<usize>().ok().map(TrayAction::SwitchCatalog);
    }
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

/// Build the tray menu (shared by [`build_tray`] and [`refresh_catalogs`]):
/// "Show Fotobase", a separator, "Pause Watching"/"Resume Watching"
/// (updated in place as the state flips), another separator, the catalog
/// switcher submenu (TASK-03-024, when more than one catalog is
/// registered), and "Quit".
fn build_menu(app: &AppHandle, toggle: &MenuItem<Wry>) -> Result<Menu<Wry>, tauri::Error> {
    use tauri::menu::{CheckMenuItem, Submenu};
    let show = MenuItem::with_id(app, MENU_SHOW, "Show Fotobase", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    // Catalog switcher (TASK-03-024): one check item per registered
    // catalog, the active one ticked. Resolved from the registry at build
    // time so the menu always mirrors the current registry.
    let catalogs = catalog_menu_entries(app);

    let mut items: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = vec![&show, &sep1, toggle, &sep2];
    let mut catalog_items: Vec<CheckMenuItem<Wry>> = Vec::new();
    let submenu;
    if catalogs.len() > 1 {
        submenu = Submenu::with_id(app, "fb-tray-catalogs", "Switch Catalog", true)?;
        for (i, (name, is_current)) in catalogs.iter().enumerate() {
            let item = CheckMenuItem::with_id(
                app,
                format!("{MENU_CATALOG_PREFIX}{i}"),
                name.clone(),
                true,
                *is_current,
                None::<&str>,
            )?;
            catalog_items.push(item);
        }
        let refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> =
            catalog_items.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<Wry>).collect();
        submenu.append_items(&refs)?;
        items.push(&submenu);
    }
    items.push(&quit_item);
    Menu::with_items(app, &items)
}

/// The (name, is-current) pairs the switcher submenu shows, from the live
/// registry; empty when there is no app state (unit-test seam).
fn catalog_menu_entries(app: &AppHandle) -> Vec<(String, bool)> {
    let Some(state) = app.try_state::<crate::AppState>() else { return Vec::new() };
    let Ok(registry) = state.registry.lock() else { return Vec::new() };
    let current = state.catalog_path();
    registry.catalogs.iter().map(|c| (c.name.clone(), catalog_db_path(&c.dir) == current)).collect()
}

/// The catalog name at a registry index, resolved at menu-event time.
fn catalog_name_at(app: &AppHandle, idx: usize) -> Option<String> {
    let state = app.try_state::<crate::AppState>()?;
    let registry = state.registry.lock().ok()?;
    registry.catalogs.get(idx).map(|c| c.name.clone())
}

/// Build the tray icon + menu and wire the menu events.
pub fn build_tray(app: &AppHandle) -> Result<TrayIcon, tauri::Error> {
    let toggle = MenuItem::with_id(app, MENU_TOGGLE_PAUSE, "Pause Watching", true, None::<&str>)?;
    let menu = build_menu(app, &toggle)?;

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
            // TASK-03-024: switch catalogs from the tray. The index is
            // resolved against the registry at event time; the switch runs
            // in the background (it drains the job queue) and the menu is
            // rebuilt at the end so the checkmark follows.
            Some(TrayAction::SwitchCatalog(idx)) => {
                if let Some(name) = catalog_name_at(app, idx) {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = crate::switch_catalog_by_name(&app, &name).await {
                            tracing::warn!(catalog = %name, error = %e, "tray catalog switch failed");
                        }
                    });
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

/// Rebuild the tray menu after the registry changed (TASK-03-024): the
/// switcher submenu mirrors the catalog list and the active catalog's
/// checkmark follows the switch. Reuses the live pause/resume item so its
/// label keeps updating in place.
pub fn refresh_catalogs(app: &AppHandle) {
    let Some(ui) = app.try_state::<TrayUi>() else { return };
    match build_menu(app, &ui.toggle) {
        Ok(menu) => {
            let _ = ui.tray.set_menu(Some(menu));
        }
        Err(e) => tracing::warn!(error = %e, "tray menu rebuild failed"),
    }
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
    if !close_intercepted(app) {
        return false;
    }
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    true
}

/// The decision part of [`handle_close_request`]: does close-to-tray apply?
/// Exposed separately because the e2e seam ([`crate::close_main_window`])
/// runs the same interception but must NOT hide the window — WebKitWebDriver
/// cannot execute against an unmapped (hidden) view, so the spec's window
/// stays mapped while the interception and background-watching behavior are
/// still what the e2e proves. The hide itself is a single `win.hide()`.
pub fn close_intercepted(app: &AppHandle) -> bool {
    app.try_state::<crate::AppState>()
        .and_then(|state| state.settings.lock().ok().map(|s| s.close_to_tray))
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_to_actions() {
        assert_eq!(tray_action(MENU_SHOW), Some(TrayAction::Show));
        assert_eq!(tray_action(MENU_TOGGLE_PAUSE), Some(TrayAction::TogglePause));
        assert_eq!(tray_action(MENU_QUIT), Some(TrayAction::Quit));
        // TASK-03-024: catalog switch items decode their registry index.
        assert_eq!(tray_action("fb-tray-catalog-0"), Some(TrayAction::SwitchCatalog(0)));
        assert_eq!(tray_action("fb-tray-catalog-12"), Some(TrayAction::SwitchCatalog(12)));
        // Malformed indices and unknown ids are ignored.
        assert_eq!(tray_action("fb-tray-catalog-x"), None);
        assert_eq!(tray_action("fb-tray-catalog-"), None);
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
