//! Fotobase Tauri v2 shell: commands, events, URI scheme handlers (PLAN §3).
//!
//! Invariants (copilot-instructions.md): originals are read-only except on
//! explicit Save; images never travel over IPC (thumb:// / preview:// serve
//! from disk cache); heavy work runs in the `fotobase-jobs` queue.

use std::path::PathBuf;
use std::sync::Mutex;

use fotobase_catalog::Catalog;
use tauri::Manager;

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase";

/// Lazily-opened catalog shared by IPC commands (TASK-01-008). The catalog is
/// a rebuildable cache (PLAN §5); nothing here writes user data.
struct AppState {
    catalog_path: PathBuf,
    catalog: Mutex<Option<Catalog>>,
}

/// Live FTS search (TASK-01-008, PLAN §5): returns matching file ids; the UI
/// renders them. No image bytes ever cross IPC.
#[tauri::command]
fn search_files(
    query: String,
    limit: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<i64>, String> {
    let mut guard = state.catalog.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))?;
    if guard.is_none() {
        let cat = Catalog::open(&state.catalog_path).map_err(|e| format!("open catalog: {e}"))?;
        *guard = Some(cat);
    }
    guard
        .as_ref()
        .expect("catalog just inserted")
        .search_files(&query, limit.unwrap_or(200))
        .map_err(|e| e.to_string())
}

/// Trivial IPC command so the frontend wiring is validated from day one.
#[tauri::command]
fn ping() -> &'static str {
    "pong"
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        // TASK-01-003: signed updater (PLAN §4). Signature verification of
        // downloaded artifacts is done by the plugin itself (minisign against
        // the pubkey pinned in tauri.conf.json) and cannot be disabled.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            app.manage(AppState {
                catalog_path: data_dir.join("catalog.db"),
                catalog: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![ping, search_files])
        .run(tauri::generate_context!())
        .expect("error while running fotobase");
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_identity() {
        assert_eq!(super::CRATE_NAME, "fotobase");
    }
}
