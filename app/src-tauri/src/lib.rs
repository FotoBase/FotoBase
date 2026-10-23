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

pub mod associations;
pub mod file_menu;
pub mod folder_manager;
pub mod import;
pub mod perf_probe;
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

/// Dedup guard for the lazy hash backfill: at most one backfill thread at a
/// time. A scan that lands while one is running is covered — the running
/// loop keeps scanning for unhashed rows until a pass makes no progress.
static HASH_BACKFILL_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Lazily-opened catalog shared by IPC commands (TASK-01-008). The catalog is
/// a rebuildable cache (PLAN §5); nothing here writes user data.
struct AppState {
    /// App handle for event emission from non-command contexts (the
    /// watcher's scan-completion hook lives on its own thread).
    app_handle: tauri::AppHandle,
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
    /// Files the OS opened with Fotobase at launch (TASK-01-025), drained
    /// once by the frontend's `take_open_files` and routed into the viewer.
    pending_open: Mutex<Vec<String>>,
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

/// [`with_catalog`] for the rare mutating commands (TASK-01-021): the `Mutex`
/// guard hands out `&mut Catalog` so a command can drop rows. Mutations stay
/// behind the same lazy-open, single-connection path as the reads.
fn with_catalog_mut<T>(
    state: &AppState,
    f: impl FnOnce(&mut Catalog) -> Result<T, fotobase_catalog::CatalogError>,
) -> Result<T, String> {
    let mut guard = state.catalog.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))?;
    if guard.is_none() {
        let cat = Catalog::open(&state.catalog_path).map_err(|e| format!("open catalog: {e}"))?;
        *guard = Some(cat);
    }
    f(guard.as_mut().expect("catalog just inserted")).map_err(|e| e.to_string())
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
        let app_handle = state.app_handle.clone();
        // Watcher-driven scans announce catalog changes on `library-changed`
        // (PLAN §2A): the frontend re-lists, so a cold-start scan — or files
        // dropped into a watched folder while the app runs — shows up in
        // the grid without any user action. Without this, a scan that lands
        // after startup never reaches the UI (perf decision run: the grid
        // scrolled EMPTY, so no thumb:// request ever happened).
        let catalog_path = state.catalog_path.clone();
        let hook = std::sync::Arc::new(move |_report: &fotobase_scanner::ScanReport| {
            let _ = app_handle.emit("library-changed", ());
            spawn_hash_backfill(catalog_path.clone(), app_handle.clone());
        });
        let fm = folder_manager::FolderManager::open_with(
            &state.catalog_path,
            fotobase_scanner::DEFAULT_DEBOUNCE,
            Some(hook),
        )?;
        *guard = Some(fm);
    }
    f(guard.as_ref().expect("folder manager just inserted"))
}

/// Lazy content-hash backfill driver (PLAN §10 "hash lazily"; perf decision
/// run: the grid listed NULL-hash rows, so no `thumb://` URI was ever built
/// and `first_thumb_serve_ms` never fired). Spawned after every scan that
/// changed the catalog — and after a rebuild — it hashes unhashed rows in
/// bounded batches on its own WAL connection (never the shared one, never
/// under a lock), re-announcing `library-changed` per batch so the grid
/// picks up hashes progressively: path order matches grid order, so the
/// visible top of the library fills first. At most one runs at a time; a
/// scan landing mid-run is covered because the loop keeps going until a
/// pass makes no progress, plus one settle pass for rows that landed
/// between the final check and the flag reset.
fn spawn_hash_backfill(catalog_path: PathBuf, app: tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    if HASH_BACKFILL_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        let Ok(mut cat) = Catalog::open(&catalog_path) else {
            HASH_BACKFILL_RUNNING.store(false, Ordering::SeqCst);
            return;
        };
        let cancel = std::sync::atomic::AtomicBool::new(false);
        loop {
            match fotobase_scanner::backfill_hashes(&mut cat, 64, &cancel) {
                // Error: the catalog connection is broken; a later scan
                // re-arms the backfill. Zero: done — one settle pass after
                // a pause catches rows that landed right behind the check.
                Ok(0) => {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    if matches!(fotobase_scanner::backfill_hashes(&mut cat, 64, &cancel), Ok(0)) {
                        break;
                    }
                }
                Ok(_) => {
                    // Best-effort: a slow UI must never stall the backfill.
                    let _ = app.emit("library-changed", ());
                }
                Err(_) => break,
            }
        }
        HASH_BACKFILL_RUNNING.store(false, Ordering::SeqCst);
    });
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

/// `thumb-rendered` event payload: a queued render landed in the disk
/// cache (outcome `Done(Ok)`). The frontend bumps its per-hash version
/// and re-requests `thumb://<hash>?v=N` — without this, a cold-start grid
/// holds the miss placeholder forever (`no-store` alone does not make an
/// already-rendered <img> re-fetch).
#[derive(Clone, Serialize)]
struct ThumbRendered {
    hash: String,
    scale: u32,
}

fn thumb_response(reply: thumb::ThumbReply) -> tauri::http::Response<Vec<u8>> {
    // A real (cached) JPEG is keyed by content hash: immutable, cache
    // forever. The placeholder is NOT cacheable — it stands in for a
    // thumbnail still being rendered, and pinning it in the WebView cache
    // would leave a cold-start grid gray forever (miss observed on the
    // perf decision run: `first_thumb_serve_ms` never fired).
    let cache_control =
        if reply.content_type == "image/jpeg" { "max-age=31536000, immutable" } else { "no-store" };
    tauri::http::Response::builder()
        .status(reply.status)
        .header("content-type", reply.content_type)
        .header("cache-control", cache_control)
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

// --- File menu / folder context menu (TASK-01-021, PLAN §2) -----------------

/// "Locate on disk" (File menu / Folder context menu): reveal a photo or
/// folder in the OS file manager, with the item selected. Pure OS adapter —
/// nothing in the catalog or on disk changes.
#[tauri::command]
fn reveal_path(path: String) -> Result<(), String> {
    file_menu::reveal_path(&path)
}

/// "Delete from disk" (File menu): move one photo to the OS trash (never a
/// permanent delete) and drop its catalog row. The UI confirms first; the
/// trash happens before the row goes, so a failed trash leaves the library
/// untouched (§3 rule 1: originals change only on the user's explicit
/// delete).
#[tauri::command]
fn delete_to_trash(file_id: i64, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let path =
        with_catalog(&state, |cat| cat.photo_info(file_id).map(|info| info.map(|i| i.path)))?;
    let path = path.ok_or_else(|| format!("unknown photo id: {file_id}"))?;
    let err = |e: String| fotobase_catalog::CatalogError::Corrupted(e);
    with_catalog_mut(&state, |cat| file_menu::delete_to_trash(file_id, &path, cat).map_err(err))
}

/// "Refresh thumbnails" (Folder context menu): invalidate the folder's
/// cached thumbnails, then re-queue their renders at background priority in
/// the bounded queue (§3 rule 4). The queue is drained first so no in-flight
/// render can write a stale entry back after the delete — the same safe
/// order `clear_cache` uses. Returns how many jobs were re-queued.
#[tauri::command]
async fn refresh_folder_thumbnails(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<usize, String> {
    let cache_dir = state.cache_dir.clone();
    let queue = state.thumbs.clone();
    queue.drain().await;
    with_catalog(&state, |cat| {
        file_menu::refresh_folder_thumbnails(cat, &cache_dir, &queue, &path)
            .map_err(fotobase_catalog::CatalogError::Corrupted)
    })
}

/// "Exit" (File menu): quit cleanly. Pending jobs are drained first —
/// pending ones cancelled, in-flight ones cancelled and waited out — so the
/// process never dies in the middle of a cache write or a file copy
/// (imports observe the cancellation and leave no half-copied file). All
/// cache writes are atomic `.tmp` + rename anyway; this just makes exit
/// deterministic.
#[tauri::command]
async fn exit_app(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let thumbs = state.thumbs.clone();
    let rebuilds = state.rebuilds.clone();
    let imports = state.imports.clone();
    thumbs.drain().await;
    rebuilds.drain().await;
    imports.drain().await;
    app.exit(0);
    Ok(())
}

/// Trivial IPC command so the frontend wiring is validated from day one.
#[tauri::command]
fn ping() -> &'static str {
    "pong"
}

// --- File associations (TASK-01-025, PLAN §2M) -------------------------------

/// Registration status of every supported image type, for the Options
/// dialog's association list.
#[tauri::command]
fn list_associations() -> Vec<associations::AssociationStatus> {
    associations::status()
}

/// Register Fotobase as the OS default viewer for the given image types.
/// The prior handler of each type is captured first, so unregistering can
/// restore exactly what the OS had before.
#[tauri::command]
fn register_associations(
    exts: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("current exe: {e}"))?;
    associations::register(&exts, &exe, &state.data_dir)
}

/// Unregister the given image types, restoring each captured prior handler.
#[tauri::command]
fn unregister_associations(
    exts: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("current exe: {e}"))?;
    associations::unregister(&exts, &exe, &state.data_dir)
}

/// Files the OS passed at launch (TASK-01-025): drained exactly once by the
/// frontend, which resolves each path against the catalog and opens the
/// viewer. Unknown/uncataloged paths are silently dropped.
#[tauri::command]
fn take_open_files(state: tauri::State<'_, AppState>) -> Vec<String> {
    state.pending_open.lock().map(|mut guard| std::mem::take(&mut *guard)).unwrap_or_default()
}

/// Resolve one on-disk path to a grid item (TASK-01-025): the OS-opened
/// path is matched against the catalog, and the viewer opens on that row.
#[tauri::command]
fn find_file_by_path(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<fotobase_catalog::FileInfo>, String> {
    with_catalog(&state, |cat| cat.file_by_path(&path))
}

/// Perf-harness probe (TASK-01-026): the §2 timestamps the WebDriver
/// harness polls for. `None` in every normal launch — the hook is inert
/// unless the harness armed the app through the environment.
#[tauri::command]
fn perf_probe() -> Option<perf_probe::Snapshot> {
    perf_probe::snapshot()
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
            // `?s=2` selects the HiDPI (1024px) cache entry (TASK-01-024).
            let req = thumb::request_from_uri(request.uri());
            let reply = match &req {
                Some(req) => thumb::resolve_thumb(req, &state.cache_dir),
                None => thumb::placeholder(),
            };
            // A real (cached) JPEG went out: the protocol's
            // time-to-first-thumbnail endpoint (TASK-01-026). Recorded before
            // the placeholder handling below — a hit is neither.
            if reply.content_type == "image/jpeg" {
                perf_probe::note_first_thumb();
            }
            // Placeholder went out (miss or malformed key): best-effort
            // trigger of the render that will fill the cache for next time.
            if reply.content_type == "image/png" {
                if let Some(req) = &req {
                    let handles = with_catalog(&state, |cat| {
                        Ok::<_, fotobase_catalog::CatalogError>(thumb::enqueue_missing_thumbs(
                            req,
                            cat,
                            &state.cache_dir,
                            &state.thumbs,
                        ))
                    })
                    .unwrap_or_default();
                    if !handles.is_empty() {
                        // When the render lands, tell the WebView: it
                        // re-requests with a fresh `?v=N` (the placeholder
                        // it holds is `no-store`, but the <img> src must
                        // still change to trigger the re-request).
                        let app = ctx.app_handle().clone();
                        let hash = req.hash.clone();
                        let scale = req.scale;
                        for handle in handles {
                            let app = app.clone();
                            let hash = hash.clone();
                            tauri::async_runtime::spawn(async move {
                                if matches!(
                                    *handle.result().await,
                                    fotobase_jobs::Outcome::Done(Ok(_))
                                ) {
                                    let _ =
                                        app.emit("thumb-rendered", ThumbRendered { hash, scale });
                                }
                            });
                        }
                    }
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
            // OS-opened files (TASK-01-025): whatever the OS passed on the
            // command line that is an existing supported image is parked
            // for the frontend to pick up and route into the viewer.
            let pending_open: Vec<String> =
                associations::collect_open_paths(&std::env::args().skip(1).collect::<Vec<_>>());
            app.manage(AppState {
                app_handle: app.handle().clone(),
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
                pending_open: Mutex::new(pending_open),
            });
            // Perf-harness hook (TASK-01-026, protocol §1): inert unless the
            // harness armed the app through the environment. Cold start with
            // an empty catalog + cache is guaranteed by the harness pointing
            // the app's data dir at a fresh directory.
            if let Some(cfg) = perf_probe::arm_from_env() {
                perf_probe::begin(cfg.expected);
                let fixture = cfg.fixture.to_string_lossy().into_owned();
                let state = app.state::<AppState>();
                if let Err(e) = with_folder_manager(&state, |fm| {
                    fm.set_mode(
                        &fixture,
                        folder_manager::parse_mode("always").expect("static mode"),
                    )
                }) {
                    tracing::warn!("perf harness: arming fixture folder failed: {e}");
                }
                perf_probe::spawn_completion_watcher(state.catalog_path.clone(), cfg.expected);
            }
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
            reveal_path,
            delete_to_trash,
            refresh_folder_thumbnails,
            exit_app,
            start_import,
            cancel_import,
            check_for_updates,
            install_pending_update,
            list_associations,
            register_associations,
            unregister_associations,
            take_open_files,
            find_file_by_path,
            perf_probe
        ])
        .build(tauri::generate_context!())
        .expect("error while building fotobase")
        .run(handle_run_event);
}

/// Run-loop events (TASK-01-025): on macOS, files the OS opens with a
/// *running* Fotobase arrive as `RunEvent::Opened` (LaunchServices), not
/// argv. They land in the same pending queue the frontend drains at
/// startup, and are announced on the `open-file` event for the live shell.
fn handle_run_event(app: &tauri::AppHandle, event: tauri::RunEvent) {
    #[cfg(target_os = "macos")]
    if let tauri::RunEvent::Opened { urls } = event {
        let paths: Vec<String> = urls
            .iter()
            .filter_map(|u| u.to_file_path().ok())
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|p| associations::is_supported_image_path(p))
            .collect();
        if paths.is_empty() {
            return;
        }
        if let Some(state) = app.try_state::<AppState>() {
            if let Ok(mut guard) = state.pending_open.lock() {
                guard.extend(paths.iter().cloned());
            }
        }
        let _ = app.emit("open-file", &paths);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, event);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_identity() {
        assert_eq!(super::CRATE_NAME, "fotobase");
    }

    #[test]
    fn thumb_response_cache_control_split_hits_from_placeholders() {
        // Content-hash-keyed JPEGs are immutable; the placeholder is NOT —
        // pinning it in the WebView cache would leave a cold-start grid
        // gray forever (perf decision run: first_thumb_serve never fired).
        let hit = super::thumb_response(super::thumb::ThumbReply {
            status: 200,
            content_type: "image/jpeg",
            body: vec![0xFF],
        });
        assert_eq!(hit.headers().get("cache-control").unwrap(), "max-age=31536000, immutable");

        let placeholder = super::thumb_response(super::thumb::placeholder());
        assert_eq!(placeholder.headers().get("cache-control").unwrap(), "no-store");
    }
}
