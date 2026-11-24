//! Mobile stand-in for the desktop tray module (TASK-09-001).
//!
//! Mobile has no system tray and no close-to-tray: the OS owns the app
//! lifecycle. The shared call sites in `lib.rs` keep compiling against the
//! same function names; each is a no-op or "not intercepted".

use tauri::AppHandle;

/// Exit the process; the e2e `quit_from_tray` seam stays valid.
pub fn quit(app: &AppHandle) {
    app.exit(0);
}

pub fn refresh_catalogs(_app: &AppHandle) {}

/// Closing is never intercepted on mobile.
pub fn handle_close_request(_app: &AppHandle) -> bool {
    false
}

pub fn close_intercepted(_app: &AppHandle) -> bool {
    false
}
