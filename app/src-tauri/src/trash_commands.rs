//! Hidden-flag and catalog-trash glue for the shell (TASK-03-009,
//! TASK-03-012). The mutation logic lives in `fotobase_catalog`; this
//! applies the user's settings to a catalog connection: "show hidden" for
//! its read paths and the undo-window sweep.

use fotobase_catalog::Catalog;

use crate::settings::Settings;

/// Apply `settings` to the shell's catalog connection: toggle "show
/// hidden", and drop trash entries older than the configured undo window.
/// Called when the catalog is opened (the startup sweep) and whenever the
/// settings change. Returns how many trash entries expired.
pub fn apply_settings(cat: &mut Catalog, settings: &Settings) -> Result<usize, String> {
    cat.set_show_hidden(settings.show_hidden);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    cat.sweep_trash(settings.trash_window_days, now).map_err(|e| format!("sweep trash: {e}"))
}
