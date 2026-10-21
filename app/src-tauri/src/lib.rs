//! Fotobase Tauri v2 shell: commands, events, URI scheme handlers (PLAN §3).
//!
//! Invariants (copilot-instructions.md): originals are read-only except on
//! explicit Save; images never travel over IPC (thumb:// / preview:// serve
//! from disk cache); heavy work runs in the `fotobase-jobs` queue.

use std::path::PathBuf;
use std::sync::Mutex;

use fotobase_catalog::Catalog;
use serde::Serialize;
use tauri::Manager;
use tauri_plugin_updater::UpdaterExt;

pub mod rebuild;
pub mod settings;
pub mod thumb;

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase";

/// Capacity of the thumbnail render queue: bounded, per PLAN §3 rule 4.
const THUMB_QUEUE_CAPACITY: usize = 64;

/// Capacity of the rebuild queue: bounded, per PLAN §3 rule 4. Rebuilds
/// coalesce on one key, so capacity 2 covers a queued + a running rebuild.
const REBUILD_QUEUE_CAPACITY: usize = 2;

/// Lazily-opened catalog shared by IPC commands (TASK-01-008). The catalog is
/// a rebuildable cache (PLAN §5); nothing here writes user data.
struct AppState {
    /// App data dir: settings.json lives here.
    data_dir: PathBuf,
    catalog_path: PathBuf,
    catalog: Mutex<Option<Catalog>>,
    /// Thumbnail cache directory backing the `thumb://` scheme.
    cache_dir: PathBuf,
    /// Bounded queue for cache-miss thumbnail renders (TASK-01-011).
    thumbs: fotobase_jobs::Queue<thumb::ThumbOutcome>,
    /// Bounded queue for the rebuild-catalog job (TASK-01-023); one key,
    /// so concurrent requests coalesce onto the running rebuild.
    rebuilds: fotobase_jobs::Queue<rebuild::RebuildReport>,
    /// Options-dialog settings (TASK-01-022); mirrored in settings.json.
    settings: Mutex<settings::Settings>,
    /// Update feed artifact found by the last check, awaiting the user's
    /// install decision (check-but-ask) or immediate install (auto).
    pending_update: Mutex<Option<tauri_plugin_updater::Update>>,
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

/// The library grid's items (TASK-01-013, PLAN §4): id + filename + content
/// hash. The hash is what the grid turns into a `thumb://` URI — pixels are
/// fetched by the WebView from the disk cache through the scheme handler,
/// never over IPC.
#[tauri::command]
fn list_files(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<fotobase_catalog::FileInfo>, String> {
    with_catalog(&state, |cat| cat.list_files(usize::MAX))
}

/// Visible-range reporting (TASK-01-013, PLAN §3 rule 4): the grid sends the
/// hashes currently on screen (debounced); those thumbnails' queued render
/// jobs are bumped to visible-first priority. Malformed hashes are ignored —
/// they can never name a cache entry. Returns how many queued jobs moved.
#[tauri::command]
fn prioritize_visible(hashes: Vec<String>, state: tauri::State<'_, AppState>) -> usize {
    hashes
        .iter()
        .filter(|h| thumb::is_cache_key(h))
        .filter(|h| state.thumbs.reprioritize(h.as_str(), fotobase_jobs::Priority::Visible))
        .count()
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

// --- Options dialog (TASK-01-022, PLAN §2 General tab) ----------------------

/// Current settings. The frontend reads these when opening the dialog.
#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> Result<settings::Settings, String> {
    let guard = state.settings.lock().map_err(|e| format!("settings mutex poisoned: {e}"))?;
    Ok(guard.clone())
}

/// Persist new settings atomically, then swap them into the shared state.
/// The file write happens first: if it fails, the in-memory state is not
/// left diverging from disk.
#[tauri::command]
fn set_settings(new: settings::Settings, state: tauri::State<'_, AppState>) -> Result<(), String> {
    new.save(&state.data_dir).map_err(|e| format!("save settings: {e}"))?;
    let mut guard = state.settings.lock().map_err(|e| format!("settings mutex poisoned: {e}"))?;
    *guard = new;
    Ok(())
}

/// "Clear cache files": drain the render queue, then empty the thumbnail
/// cache. Heavy work, so it runs as an async command; the library and the
/// catalog are never touched.
#[tauri::command]
async fn clear_cache(state: tauri::State<'_, AppState>) -> Result<u64, String> {
    // Clone before the await: a borrow derived from `State` must not be
    // held across an await point (the command future has to stay Send).
    let cache_dir = state.cache_dir.clone();
    let queue = state.thumbs.clone();
    thumb::clear_thumbnail_cache(&cache_dir, &queue).await.map_err(|e| format!("clear cache: {e}"))
}

/// "Rebuild catalog" (TASK-01-023, PLAN §2L, §5): full re-scan of every
/// watched folder with the incremental skip disabled, repopulating derived
/// rows. Heavy work runs in the bounded job queue (§3 rule 4) on a blocking
/// thread, so the UI stays responsive; progress streams on the
/// `rebuild-progress` event. Concurrent requests coalesce onto the running
/// rebuild instead of stacking a second one.
#[tauri::command]
async fn rebuild_catalog(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<rebuild::RebuildReport, String> {
    let queue = state.rebuilds.clone();
    let job_app = app.clone();
    let handle = queue
        .try_enqueue(
            "rebuild",
            fotobase_jobs::Priority::Background,
            fotobase_jobs::job(move |_ctx| {
                let app = job_app.clone();
                async move {
                    let joined =
                        tauri::async_runtime::spawn_blocking(move || rebuild::run(&app)).await;
                    match joined {
                        Ok(Ok(report)) => fotobase_jobs::Outcome::Done(report),
                        Ok(Err(e)) => fotobase_jobs::Outcome::Failed(e),
                        Err(e) => fotobase_jobs::Outcome::Failed(format!("rebuild task: {e}")),
                    }
                }
            }),
        )
        .map_err(|e| format!("enqueue rebuild: {e}"))?;
    match handle.result().await.as_ref() {
        fotobase_jobs::Outcome::Done(report) => Ok(report.clone()),
        fotobase_jobs::Outcome::Cancelled => Err("rebuild cancelled".into()),
        fotobase_jobs::Outcome::Failed(e) => Err(e.clone()),
    }
}

/// What an update check reports back to the UI.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateCheck {
    /// `false` when the mode is Off: nothing was contacted, ever.
    pub checked: bool,
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
}

/// Check the update feed, honoring the user's update mode (TASK-01-022):
/// `off` returns without any network access — §1 allows update checks only
/// when the user has not disabled them. A found artifact is held in state
/// for [`install_pending_update`]; it is never downloaded before then.
#[tauri::command]
async fn check_for_updates(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<UpdateCheck, String> {
    let mode = {
        let guard = state.settings.lock().map_err(|e| format!("settings mutex poisoned: {e}"))?;
        guard.update_mode
    };
    if !mode.phones_home() {
        return Ok(UpdateCheck { checked: false, available: false, version: None, notes: None });
    }
    let update = app
        .updater()
        .map_err(|e| format!("updater unavailable: {e}"))?
        .check()
        .await
        .map_err(|e| format!("update check failed: {e}"))?;
    match update {
        Some(update) => {
            let check = UpdateCheck {
                checked: true,
                available: true,
                version: Some(update.version.clone()),
                notes: update.body.clone(),
            };
            *state
                .pending_update
                .lock()
                .map_err(|e| format!("pending update mutex poisoned: {e}"))? = Some(update);
            Ok(check)
        }
        None => Ok(UpdateCheck { checked: true, available: false, version: None, notes: None }),
    }
}

/// Download + install the update found by [`check_for_updates`]. The
/// artifact is signature-verified by the updater plugin (minisign against
/// the pinned public key) before it ever runs.
#[tauri::command]
async fn install_pending_update(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let update = state
        .pending_update
        .lock()
        .map_err(|e| format!("pending update mutex poisoned: {e}"))?
        .take();
    let Some(update) = update else { return Ok(()) };
    update
        .download_and_install(
            |_chunk, _total| {},
            || tracing::info!("update downloaded; restarting to install"),
        )
        .await
        .map_err(|e| format!("update install failed: {e}"))?;
    Ok(())
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
            let rebuilds = tauri::async_runtime::block_on(async {
                fotobase_jobs::Queue::new(REBUILD_QUEUE_CAPACITY)
            });
            let persisted = settings::Settings::load(&data_dir);
            app.manage(AppState {
                data_dir: data_dir.clone(),
                catalog_path: data_dir.join("catalog.db"),
                catalog: Mutex::new(None),
                cache_dir,
                thumbs,
                rebuilds,
                settings: Mutex::new(persisted),
                pending_update: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            search_files,
            list_files,
            prioritize_visible,
            get_settings,
            set_settings,
            clear_cache,
            rebuild_catalog,
            check_for_updates,
            install_pending_update
        ])
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
