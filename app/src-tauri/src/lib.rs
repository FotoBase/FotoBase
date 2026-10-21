//! Fotobase Tauri v2 shell: commands, events, URI scheme handlers (PLAN §3).
//!
//! Invariants (copilot-instructions.md): originals are read-only except on
//! explicit Save; images never travel over IPC (thumb:// / preview:// serve
//! from disk cache); heavy work runs in the `fotobase-jobs` queue.

use std::path::PathBuf;
use std::sync::Mutex;

use fotobase_catalog::Catalog;
use tauri::Manager;

pub mod thumb;

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase";

/// Capacity of the thumbnail render queue: bounded, per PLAN §3 rule 4.
const THUMB_QUEUE_CAPACITY: usize = 64;

/// Lazily-opened catalog shared by IPC commands (TASK-01-008). The catalog is
/// a rebuildable cache (PLAN §5); nothing here writes user data.
struct AppState {
    catalog_path: PathBuf,
    catalog: Mutex<Option<Catalog>>,
    /// Thumbnail cache directory backing the `thumb://` scheme.
    cache_dir: PathBuf,
    /// Bounded queue for cache-miss thumbnail renders (TASK-01-011).
    thumbs: fotobase_jobs::Queue<thumb::ThumbOutcome>,
}

/// Lock-and-open the shared catalog; commands and the scheme handler share
/// one lazily-opened connection.
fn with_catalog<T>(
    state: &AppState,
    f: impl FnOnce(&Catalog) -> Result<T, fotobase_catalog::CatalogError>,
) -> Result<T, String> {
    let mut guard = state.catalog.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))?;
    if guard.is_none() {
        let cat = Catalog::open(&state.catalog_path).map_err(|e| format!("open catalog: {e}"))?;
        *guard = Some(cat);
    }
    f(guard.as_ref().expect("catalog just inserted")).map_err(|e| e.to_string())
}

/// Live FTS search (TASK-01-008, PLAN §5): returns matching file ids; the UI
/// renders them. No image bytes ever cross IPC.
#[tauri::command]
fn search_files(
    query: String,
    limit: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<i64>, String> {
    with_catalog(&state, |cat| cat.search_files(&query, limit.unwrap_or(200)))
}

/// `thumb://` scheme handler (TASK-01-011, PLAN §3 rule 3): serves cached
/// thumbnails from the disk cache to the WebView — zero image bytes cross
/// IPC. A cache miss returns the placeholder image and triggers a render job
/// in the bounded queue; it is never an error dialog.
const THUMB_PROTOCOL: &str = "thumb";

fn thumb_response(reply: thumb::ThumbReply) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(reply.status)
        .header("content-type", reply.content_type)
        .header("cache-control", "max-age=31536000, immutable") // content-hash keyed
        .header("access-control-allow-origin", "*")
        .body(reply.body)
        .expect("static thumb response parts are valid")
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
        .register_uri_scheme_protocol(THUMB_PROTOCOL, |ctx, request| {
            let state = ctx.app_handle().state::<AppState>();
            let hash = thumb::hash_from_uri(request.uri());
            let reply = match hash.as_deref() {
                Some(hash) => thumb::resolve_thumb(hash, &state.cache_dir),
                None => thumb::placeholder(),
            };
            // Placeholder went out (miss or malformed key): best-effort
            // trigger of the render that will fill the cache for next time.
            if reply.content_type == "image/png" {
                if let Some(hash) = hash.as_deref() {
                    let _ = with_catalog(&state, |cat| {
                        thumb::enqueue_missing_thumbs(hash, cat, &state.cache_dir, &state.thumbs);
                        Ok(())
                    });
                }
            }
            thumb_response(reply)
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let cache_dir = data_dir.join("thumbnails");
            std::fs::create_dir_all(&cache_dir)?;
            // Queue::new spawns its worker; block_on supplies the runtime
            // context (setup runs outside an async fn).
            let thumbs = tauri::async_runtime::block_on(async {
                fotobase_jobs::Queue::new(THUMB_QUEUE_CAPACITY)
            });
            app.manage(AppState {
                catalog_path: data_dir.join("catalog.db"),
                catalog: Mutex::new(None),
                cache_dir,
                thumbs,
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
