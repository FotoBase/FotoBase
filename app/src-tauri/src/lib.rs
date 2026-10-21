//! Fotobase Tauri v2 shell: commands, events, URI scheme handlers (PLAN §3).
//!
//! Invariants (copilot-instructions.md): originals are read-only except on
//! explicit Save; images never travel over IPC (thumb:// / preview:// serve
//! from disk cache); heavy work runs in the `fotobase-jobs` queue.

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase";

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
        .invoke_handler(tauri::generate_handler![ping])
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
