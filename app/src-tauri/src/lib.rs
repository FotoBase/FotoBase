//! Fotobase Tauri v2 shell: commands, events, URI scheme handlers (PLAN §3).
//!
//! Invariants (copilot-instructions.md): originals are read-only except on
//! explicit Save; images never travel over IPC (thumb:// / preview:// serve
//! from disk cache); heavy work runs in the `fotobase-jobs` queue.

use std::path::PathBuf;
use std::sync::Mutex;

use fotobase_catalog::Catalog;
use serde::Serialize;
use tauri::{Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

pub mod folder_manager;
pub mod import;
pub mod preview;
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

/// Capacity of the import queue: bounded, per PLAN §3 rule 4. Imports do
/// not coalesce (different sources/destinations are different work), so
/// each request gets a unique key; the single worker serializes them.
const IMPORT_QUEUE_CAPACITY: usize = 2;

/// Sequence number for unique import job keys.
static IMPORT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
    /// Bounded queue for import jobs (TASK-01-017); unique keys, the
    /// worker serializes concurrent imports.
    imports: fotobase_jobs::Queue<import::ImportReport>,
    /// Cancel token of the most recently started import (TASK-01-017):
    /// `cancel_import` trips it, the running job observes the flag.
    import_cancel: Mutex<Option<fotobase_jobs::CancelToken>>,
    /// Options-dialog settings (TASK-01-022); mirrored in settings.json.
    settings: Mutex<settings::Settings>,
    /// Update feed artifact found by the last check, awaiting the user's
    /// install decision (check-but-ask) or immediate install (auto).
    pending_update: Mutex<Option<tauri_plugin_updater::Update>>,
    /// Folder Manager backend (TASK-01-016): watcher + its own catalog
    /// connection, created lazily on the first folder-manager command (and
    /// then armed for every persisted `always` folder).
    folders: Mutex<Option<folder_manager::FolderManager>>,
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

/// Lock-and-create the Folder Manager backend; it owns a second catalog
/// connection (WAL: UI reads through [`with_catalog`] never block behind a
/// watch-triggered scan) and the `notify` watcher armed for every persisted
/// `always` folder.
fn with_folder_manager<T>(
    state: &AppState,
    f: impl FnOnce(&folder_manager::FolderManager) -> Result<T, String>,
) -> Result<T, String> {
    let mut guard =
        state.folders.lock().map_err(|e| format!("folder manager mutex poisoned: {e}"))?;
    if guard.is_none() {
        let fm = folder_manager::FolderManager::open(
            &state.catalog_path,
            fotobase_scanner::DEFAULT_DEBOUNCE,
        )?;
        *guard = Some(fm);
    }
    f(guard.as_ref().expect("folder manager just inserted"))
}

/// Live FTS search (TASK-01-008/01-018, PLAN §5): returns matching grid
/// rows (id + filename + hash), best matches first; the UI renders them
/// directly through the `thumb://` scheme. An optional folder-id list
/// narrows the match to the Folders-view selection (TASK-01-015) — search
/// and folder filter combine. No image bytes ever cross IPC.
#[tauri::command]
fn search_files(
    query: String,
    folders: Option<Vec<i64>>,
    limit: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<fotobase_catalog::FileInfo>, String> {
    let limit = limit.unwrap_or(200);
    with_catalog(&state, |cat| match folders {
        None => cat.search_files_detailed(&query, None, limit),
        Some(ids) => cat.search_files_detailed(&query, Some(&ids), limit),
    })
}

/// The library grid's items (TASK-01-013, PLAN §4): id + filename + content
/// hash. The hash is what the grid turns into a `thumb://` URI — pixels are
/// fetched by the WebView from the disk cache through the scheme handler,
/// never over IPC. An optional folder-id list (TASK-01-015) restricts the
/// result to the folders selected in the Folders view.
#[tauri::command]
fn list_files(
    folders: Option<Vec<i64>>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<fotobase_catalog::FileInfo>, String> {
    with_catalog(&state, |cat| match folders {
        None => cat.list_files(usize::MAX),
        Some(ids) => cat.list_files_in_folders(&ids, usize::MAX),
    })
}

/// Info pane payload for one photo (TASK-01-020, PLAN §2D): file properties
/// (path, size, dimensions, format, modified) plus the EXIF fields cached in
/// the `metadata` table at scan time. `None` for an unknown id. No image
/// bytes — the histogram is computed by the WebView from the `preview://`
/// bytes it already displays.
#[tauri::command]
fn photo_info(
    file_id: i64,
    state: tauri::State<'_, AppState>,
) -> Result<Option<fotobase_catalog::PhotoInfo>, String> {
    with_catalog(&state, |cat| cat.photo_info(file_id))
}

/// The Folders-view tree data (TASK-01-015, PLAN §2B): every catalog folder
/// with the year its newest file falls in. Ids + paths only — no image
/// bytes, and the UI derives display names from the paths.
#[tauri::command]
fn list_folders(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<fotobase_catalog::FolderInfo>, String> {
    with_catalog(&state, |cat| cat.list_folders())
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

/// `preview://` scheme handler (TASK-01-019, PLAN §3 rule 3): serves the
/// full-size image for the single-photo viewer, resolved hash → original
/// path → bytes straight from disk. Zero image bytes cross IPC. A miss
/// (unknown hash, vanished file) returns the placeholder image — the
/// viewer never pops an error dialog.
const PREVIEW_PROTOCOL: &str = "preview";
fn preview_response(reply: preview::PreviewReply) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(reply.status)
        .header("content-type", reply.content_type)
        .header("access-control-allow-origin", "*")
        .body(reply.body)
        .expect("static preview response parts are valid")
}

// --- Folder Manager (TASK-01-016, PLAN §2A) ---------------------------------

/// Apply a folder's scan mode: "once" scans it now and leaves it unwatched;
/// "always" hands it to the notify watcher (its closing scan brings the
/// folder into the library immediately). The mode is persisted with the
/// folder row, so a restart resumes the same watching set.
#[tauri::command]
fn set_folder_mode(
    path: String,
    mode: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mode =
        folder_manager::parse_mode(&mode).ok_or_else(|| format!("unknown scan mode: {mode}"))?;
    with_folder_manager(&state, |fm| fm.set_mode(&path, mode))
}

/// Remove a folder from the catalog (nested rows included). Files on disk
/// are never touched — the confirmation for that is the UI's job. Returns
/// how many folder rows were dropped.
#[tauri::command]
fn remove_folder(path: String, state: tauri::State<'_, AppState>) -> Result<usize, String> {
    with_folder_manager(&state, |fm| fm.remove(&path))
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

/// Import from folder / removable drive (TASK-01-017, PLAN §2A/§6 P1):
/// copies files into the catalog location with date/custom destination
/// naming, skipping cataloged duplicates when "exclude duplicates" is on
/// (the default). Heavy work runs in the bounded job queue (§3 rule 4) on
/// a blocking thread; progress streams on the `import-progress` event.
/// `cancel_import` trips the cancel token the job observes — a cancelled
/// import never leaves a half-copied file at the destination.
#[tauri::command]
async fn start_import(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    source: String,
    destination: String,
    naming: String,
    custom_pattern: Option<String>,
    exclude_duplicates: Option<bool>,
) -> Result<import::ImportReport, String> {
    let req =
        import::parse_request(&source, &destination, &naming, custom_pattern, exclude_duplicates)
            .ok_or_else(|| "invalid import request".to_string())?;

    // A fresh token per import: cancel_import always targets the import
    // the UI is currently looking at (the worker serializes imports).
    let token = fotobase_jobs::CancelToken::default();
    store_import_cancel(&state, token.clone())?;

    let queue = state.imports.clone();
    let job_app = app.clone();
    let job_catalog_path = state.catalog_path.clone();
    let seq = IMPORT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let handle = queue
        .try_enqueue(
            format!("import-{seq}"),
            fotobase_jobs::Priority::Background,
            fotobase_jobs::job(move |ctx| {
                let app = job_app.clone();
                let req = req.clone();
                let catalog_path = job_catalog_path.clone();
                let token = token.clone();
                let ctx = ctx.clone();
                async move {
                    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let joined = tauri::async_runtime::spawn_blocking(move || {
                        let emit_flag = flag.clone();
                        let emit = move |p: import::ImportProgress| {
                            // Bridge the user's cancel token (and the
                            // queue's own cancellation) onto the plain
                            // flag the engine checks.
                            import::sync_cancel(
                                &emit_flag,
                                token.is_cancelled() || ctx.is_cancelled(),
                            );
                            let _ = app.emit(import::IMPORT_EVENT, p);
                        };
                        import::run_import(
                            &catalog_path,
                            &req.source,
                            &req.dest_root,
                            &req.options,
                            &flag,
                            &emit,
                        )
                    })
                    .await;
                    match joined {
                        Ok(Ok(report)) => fotobase_jobs::Outcome::Done(report),
                        Ok(Err(e)) => fotobase_jobs::Outcome::Failed(e),
                        Err(e) => fotobase_jobs::Outcome::Failed(format!("import task: {e}")),
                    }
                }
            }),
        )
        .map_err(|e| format!("enqueue import: {e}"))?;
    match handle.result().await.as_ref() {
        fotobase_jobs::Outcome::Done(report) => Ok(report.clone()),
        fotobase_jobs::Outcome::Cancelled => Err("import cancelled".into()),
        fotobase_jobs::Outcome::Failed(e) => Err(e.clone()),
    }
}

/// Publish the new import's cancel token. A sync helper on purpose: the
/// `MutexGuard` must never exist inside the async command body, or the
/// future stops being `Send`.
fn store_import_cancel(state: &AppState, token: fotobase_jobs::CancelToken) -> Result<(), String> {
    let mut guard =
        state.import_cancel.lock().map_err(|e| format!("import cancel mutex poisoned: {e}"))?;
    *guard = Some(token);
    Ok(())
}

/// Cancel the import that is currently running or queued (TASK-01-017).
/// Best-effort: with no import in flight it is a no-op.
#[tauri::command]
fn cancel_import(state: tauri::State<'_, AppState>) {
    let token = state
        .import_cancel
        .lock()
        .map_err(|e| format!("import cancel mutex poisoned: {e}"))
        .ok()
        .and_then(|guard| guard.clone());
    if let Some(token) = token {
        token.cancel();
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
        .register_uri_scheme_protocol(PREVIEW_PROTOCOL, |ctx, request| {
            let state = ctx.app_handle().state::<AppState>();
            // Same URI grammar as thumb:// — reuse its parser.
            let hash = thumb::hash_from_uri(request.uri()).unwrap_or_default();
            let reply = with_catalog(&state, |cat| {
                Ok::<_, fotobase_catalog::CatalogError>(preview::resolve_preview(&hash, cat))
            })
            .unwrap_or_else(|_| preview::placeholder());
            preview_response(reply)
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
            let imports = tauri::async_runtime::block_on(async {
                fotobase_jobs::Queue::new(IMPORT_QUEUE_CAPACITY)
            });
            let persisted = settings::Settings::load(&data_dir);
            app.manage(AppState {
                data_dir: data_dir.clone(),
                catalog_path: data_dir.join("catalog.db"),
                catalog: Mutex::new(None),
                cache_dir,
                thumbs,
                rebuilds,
                imports,
                import_cancel: Mutex::new(None),
                settings: Mutex::new(persisted),
                pending_update: Mutex::new(None),
                folders: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            search_files,
            list_files,
            photo_info,
            list_folders,
            prioritize_visible,
            get_settings,
            set_settings,
            clear_cache,
            rebuild_catalog,
            set_folder_mode,
            remove_folder,
            start_import,
            cancel_import,
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
