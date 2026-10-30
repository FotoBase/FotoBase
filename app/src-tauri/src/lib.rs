//! Fotobase Tauri v2 shell: commands, events, URI scheme handlers (PLAN §3).
//!
//! Invariants (copilot-instructions.md): originals are read-only except on
//! explicit Save; images never travel over IPC (thumb:// / preview:// serve
//! from disk cache); heavy work runs in the `fotobase-jobs` queue.

use std::path::PathBuf;
use std::sync::Mutex;

use fotobase_catalog::{Catalog, SyncError};
use serde::Serialize;
use tauri::{Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

pub mod associations;
pub mod batch_commands;
pub mod catalog_registry;
pub mod codec;
pub mod datetime_commands;
pub mod display_profile;
pub mod emit_pacer;
pub mod file_menu;
pub mod folder_manager;
pub mod import;
pub mod perf_probe;
pub mod preview;
pub mod rating;
pub mod rebuild;
pub mod settings;
pub mod stream;
pub mod tag_commands;
pub mod thumb;
pub mod trash_commands;
pub mod tray;

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase";

pub use catalog_registry::{catalog_cache_dir, catalog_db_path};

/// Capacity of the thumbnail render queue: bounded, per PLAN §3 rule 4.
const THUMB_QUEUE_CAPACITY: usize = 64;

/// Capacity of the rebuild queue: bounded, per PLAN §3 rule 4. Rebuilds
/// coalesce on one key, so capacity 2 covers a queued + a running rebuild.
const REBUILD_QUEUE_CAPACITY: usize = 2;

/// Capacity of the import queue: bounded, per PLAN §3 rule 4. Imports do
/// not coalesce (different sources/destinations are different work), so
/// each request gets a unique key; the single worker serializes them.
const IMPORT_QUEUE_CAPACITY: usize = 2;

/// Capacity of the date/time adjustment queue: bounded, per PLAN §3 rule 4.
/// Batches differ in selection and parameters, so each gets a unique key;
/// the single worker serializes them (one sidecar writer at a time).
const DATETIME_QUEUE_CAPACITY: usize = 4;

/// Capacity of the portable-export queue (TASK-03-023): bounded, per PLAN
/// §3 rule 4; exports are cheap reads, the single worker serializes them.
const EXPORT_QUEUE_CAPACITY: usize = 2;

/// Sequence number for unique portable-export job keys.
static EXPORT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Sequence number for unique date/time job keys.
static DATETIME_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
    /// Paths of the *active* catalog (TASK-03-024): swapped by
    /// `switch_catalog` without an app restart. `Mutex` because the switch
    /// rewrites them while commands read them.
    catalog_path: Mutex<PathBuf>,
    catalog: Mutex<Option<Catalog>>,
    /// Thumbnail cache directory backing the `thumb://` scheme (per-catalog,
    /// TASK-03-024).
    cache_dir: Mutex<PathBuf>,
    /// Catalog registry (TASK-03-024): the JSON list of catalog locations
    /// under `data_dir`, separate from any catalog.
    registry: Mutex<catalog_registry::Registry>,
    /// Bounded queue for cache-miss thumbnail renders (TASK-01-011).
    thumbs: fotobase_jobs::Queue<thumb::ThumbOutcome>,
    /// Bounded queue for the rebuild-catalog job (TASK-01-023); one key,
    /// so concurrent requests coalesce onto the running rebuild.
    rebuilds: fotobase_jobs::Queue<rebuild::RebuildReport>,
    /// Bounded queue for import jobs (TASK-01-017); unique keys, the
    /// worker serializes concurrent imports.
    imports: fotobase_jobs::Queue<import::ImportReport>,
    /// Bounded queue for date/time adjustment batches (TASK-03-019).
    datetime_jobs: fotobase_jobs::Queue<fotobase_catalog::AdjustReport>,
    /// Bounded queue for portable user-data exports (TASK-03-023).
    exports: fotobase_jobs::Queue<fotobase_catalog::ExportReport>,
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
    /// Current display ICC profile the render pipeline transforms into
    /// (TASK-02-001): the OS profile on macOS/Windows, the bundled sRGB
    /// profile on Linux. Swapped in place by the profile watcher; the render
    /// jobs read it per enqueue so a switch re-renders into new cache entries.
    display_profile: Mutex<Option<Vec<u8>>>,
    /// ffmpeg frame extractor backing video thumbnails (TASK-02-004): the
    /// shipped LGPL sidecar (real binary in production, fakes in tests).
    video_extractor: std::sync::Arc<dyn fotobase_video::FrameExtractor + Send + Sync>,
    /// WebView codec matrix reported at startup (TASK-02-005): consulted by
    /// the playback decision before choosing HTML5 `<video>` vs `stream://`
    /// transcode (02-006). `None` until the frontend has probed.
    codec_support: Mutex<Option<codec::CodecSupport>>,
    /// In-flight `stream://` transcodes (TASK-02-006) keyed by file-id:
    /// `stop_stream` kills them, a superseding request kills the previous
    /// one, and a janitor reaps idle stragglers as the backstop.
    streams: stream::StreamRegistry,
}

impl AppState {
    /// Snapshot of the active catalog's database path.
    fn catalog_path(&self) -> PathBuf {
        self.catalog_path.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Snapshot of the active catalog's thumbnail-cache directory.
    fn cache_dir(&self) -> PathBuf {
        self.cache_dir.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

/// Destination profile for the render pipeline (TASK-02-001, PLAN §6 P1.5):
/// the OS display profile on macOS/Windows, the bundled sRGB profile on
/// Linux or wherever no usable display profile exists.
fn initial_display_profile() -> Option<Vec<u8>> {
    display_profile::current_display_profile()
        .or_else(|| fotobase_render::color::srgb_profile_bytes().ok())
}

/// Lock-and-open the shared catalog; commands and the scheme handler share
/// one lazily-opened connection.
fn with_catalog<T>(
    state: &AppState,
    f: impl FnOnce(&Catalog) -> Result<T, fotobase_catalog::CatalogError>,
) -> Result<T, String> {
    let mut guard = state.catalog.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))?;
    if guard.is_none() {
        *guard = Some(open_shell_catalog(state)?);
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
        *guard = Some(open_shell_catalog(state)?);
    }
    f(guard.as_mut().expect("catalog just inserted")).map_err(|e| e.to_string())
}

/// Open the shell's catalog connection with the user's settings applied:
/// "show hidden" for its read paths, and the undo-window sweep that expires
/// old trash entries (TASK-03-009, TASK-03-012). Called whenever the lazily
/// opened connection is created — the first command after startup.
fn open_shell_catalog(state: &AppState) -> Result<Catalog, String> {
    let mut cat = Catalog::open(state.catalog_path()).map_err(|e| format!("open catalog: {e}"))?;
    let settings =
        state.settings.lock().map_err(|e| format!("settings mutex poisoned: {e}"))?.clone();
    if let Err(err) = trash_commands::apply_settings(&mut cat, &settings) {
        tracing::warn!(error = %err, "applying hidden/trash settings failed");
    }
    Ok(cat)
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
        let catalog_path = state.catalog_path();
        let hook = std::sync::Arc::new(move |report: &fotobase_scanner::ScanReport| {
            let _ = app_handle.emit("library-changed", ());
            if !report.folder_ini_offered.is_empty() {
                let _ = app_handle.emit("folder-ini-offered", &report.folder_ini_offered);
            }
            spawn_hash_backfill(catalog_path.clone(), app_handle.clone());
        });
        let fm = folder_manager::FolderManager::open_with(
            &state.catalog_path(),
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
        // Announce batches time-coalesced, not per batch (TASK-01-004 perf
        // run): 50k rows at 64/batch meant ~781 `library-changed` emits in
        // ~31 s, each a full 50k-row re-list over IPC — heap churn the
        // idle-RAM bar paid for. First batch announces immediately; later
        // ones at most every 250 ms; one flush after the loop so the final
        // state is never left unannounced.
        let mut pacer = emit_pacer::EmitPacer::new(std::time::Duration::from_millis(250));
        let mut pending_announce = false;
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
                    pending_announce = true;
                    if pacer.fire(std::time::Instant::now()) {
                        let _ = app.emit("library-changed", ());
                        pending_announce = false;
                    }
                }
                Err(_) => break,
            }
        }
        if pending_announce {
            let _ = app.emit("library-changed", ());
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
/// result to the folders selected in the Folders view; `starred_only`
/// (TASK-03-005) keeps only starred photos (rating >= 5, or >= 1 when
/// ratings are enabled).
#[tauri::command]
fn list_files(
    folders: Option<Vec<i64>>,
    starred_only: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<fotobase_catalog::FileInfo>, String> {
    if starred_only == Some(true) {
        let min = rating::starred_threshold(ratings_enabled(&state)?);
        return with_catalog(&state, |cat| cat.list_starred(folders.as_deref(), min, usize::MAX));
    }
    with_catalog(&state, |cat| match folders {
        None => cat.list_files(usize::MAX),
        Some(ids) => cat.list_files_in_folders(&ids, usize::MAX),
    })
}

/// The grid's one query (TASK-03-014, PLAN §2 C): folder selection, album
/// view, live search text and the filter buttons / date slider AND-combine
/// in a single catalog query. `starred` resolves to the rating threshold for
/// the current rating mode; `taken_since` is the date slider's `YYYY-MM-DD`
/// cutoff; `sort` is `path` (default) or `capture` (TASK-03-019: chronological,
/// adjusted capture times included); `stack` (TASK-03-021) expands one stack
/// instead of showing every stack as its top. Ids and metadata only — no
/// image bytes over IPC.
#[tauri::command]
// Each argument is one named IPC field the frontend sends; bundling them
// would change the wire shape for no gain.
#[allow(clippy::too_many_arguments)]
fn query_files(
    text: Option<String>,
    folders: Option<Vec<i64>>,
    album: Option<i64>,
    starred: Option<bool>,
    videos_only: Option<bool>,
    taken_since: Option<String>,
    sort: Option<fotobase_catalog::FileSort>,
    stack: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<fotobase_catalog::FileInfo>, String> {
    let min_rating = if starred == Some(true) {
        Some(rating::starred_threshold(ratings_enabled(&state)?))
    } else {
        None
    };
    let query = fotobase_catalog::FileQuery {
        text,
        folders,
        album,
        min_rating,
        videos_only: videos_only == Some(true),
        taken_since,
        stack,
        sort: sort.unwrap_or_default(),
    };
    with_catalog(&state, |cat| cat.query_files(&query, usize::MAX))
}

fn ratings_enabled(state: &AppState) -> Result<bool, String> {
    let guard = state.settings.lock().map_err(|e| format!("settings mutex poisoned: {e}"))?;
    Ok(guard.ratings_enabled)
}

/// Star / rate one photo (TASK-03-005): `rating` 0 unstars, 5 stars, 1–4
/// only while ratings are enabled. Write-through: the sidecar is written
/// first, the catalog mirror only after that succeeded. Returns the new
/// rating so the UI can reflect what landed.
#[tauri::command]
fn set_rating(file_id: i64, rating: u8, state: tauri::State<'_, AppState>) -> Result<u8, String> {
    let enabled = ratings_enabled(&state)?;
    let mut guard = state.catalog.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))?;
    if guard.is_none() {
        *guard = Some(open_shell_catalog(&state)?);
    }
    let cat = guard.as_mut().expect("catalog just inserted");
    rating::apply_rating(cat, file_id, rating, enabled).map_err(|e| e.to_string())?;
    Ok(rating)
}

/// Captions (TASK-03-007): read a photo's caption from the catalog mirror.
#[tauri::command]
fn get_caption(file_id: i64, state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    with_catalog_sync(&state, |cat| cat.file_caption(file_id).map_err(SyncError::from))
}

/// Write a photo's caption to `dc:description` (sidecar first, then the
/// catalog mirror + FTS). A blank caption clears the field. Returns the
/// caption that landed so the UI shows what is actually stored.
#[tauri::command]
fn set_caption(
    file_id: i64,
    caption: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, String> {
    with_catalog_sync(&state, |cat| {
        cat.set_caption(file_id, &caption)?;
        cat.file_caption(file_id).map_err(SyncError::from)
    })
}

/// Outcome of an info-pane metadata edit (TASK-03-020): the per-field
/// rejections (nothing written when any), else the photo's refreshed info.
#[derive(serde::Serialize)]
struct MetadataEditResult {
    errors: Vec<fotobase_catalog::FieldError>,
    info: Option<fotobase_catalog::PhotoInfo>,
}

/// Edit one photo's capture date, GPS, artist and copyright (TASK-03-020):
/// validated first, then one sidecar write, catalog mirror second.
#[tauri::command]
fn edit_photo_metadata(
    file_id: i64,
    edit: fotobase_catalog::MetadataEdit,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<MetadataEditResult, String> {
    let result = with_catalog_sync(&state, |cat| {
        let errors = cat.edit_metadata(file_id, &edit)?;
        let info = cat.photo_info(file_id).map_err(SyncError::from)?;
        Ok(MetadataEditResult { errors, info })
    })?;
    if result.errors.is_empty() && edit.taken_at.is_some() {
        // A new capture time can reorder the grid.
        let _ = app.emit("library-changed", ());
    }
    Ok(result)
}

/// Stack photos together (TASK-03-021): a new stack (first id on top) or
/// the existing stack of one of them. Metadata only — each member's sidecar
/// gets `fb:StackId` / `fb:StackRole`, no file moves. Returns the stack id.
#[tauri::command]
fn stack_photos(
    file_ids: Vec<i64>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let stack_id = with_catalog_sync(&state, |cat| cat.stack_photos(&file_ids))?;
    let _ = app.emit("library-changed", ());
    Ok(stack_id)
}

/// Make a stack member the top of its stack (TASK-03-021).
#[tauri::command]
fn set_stack_top(
    file_id: i64,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    with_catalog_sync(&state, |cat| cat.set_stack_top(file_id))?;
    let _ = app.emit("library-changed", ());
    Ok(())
}

/// Dissolve a stack (TASK-03-021); every photo is listed on its own again.
#[tauri::command]
fn unstack(
    stack_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    with_catalog_sync(&state, |cat| cat.unstack(&stack_id))?;
    let _ = app.emit("library-changed", ());
    Ok(())
}

/// The photos of one stack for the picker overlay, top first (TASK-03-021).
#[tauri::command]
fn list_stack(
    stack_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<fotobase_catalog::FileInfo>, String> {
    with_catalog(&state, |cat| cat.stack_members(&stack_id))
}

/// RAW+JPEG pairs and edited versions the user may want to stack
/// (TASK-03-021). Nothing is stacked until the user accepts a group.
#[tauri::command]
fn stack_suggestions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<fotobase_catalog::StackSuggestion>, String> {
    let window = {
        let guard = state.settings.lock().map_err(|e| format!("settings mutex poisoned: {e}"))?;
        i64::from(guard.stack_window_secs)
    };
    with_catalog(&state, |cat| cat.stack_suggestions(window))
}

/// Folders whose folder-ini migration offer is still unanswered
/// (TASK-03-022). A scan records the offer; nothing is migrated until the
/// user accepts.
#[tauri::command]
fn folder_ini_pending_folders(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    with_catalog(&state, |cat| cat.folder_ini_pending_folders())
}

/// Accept the migration for one folder: its stars, albums and faces
/// become XMP sidecar data. The ini itself is never modified.
#[tauri::command]
fn ingest_folder_ini(
    path: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<fotobase_catalog::FolderIniReport, String> {
    let report = with_catalog_sync(&state, |cat| cat.ingest_folder_ini(&path))?;
    if report.written > 0 || report.albums_created > 0 {
        let _ = app.emit("library-changed", ());
    }
    Ok(report)
}

/// Decline the migration for one folder; scans never offer it again.
#[tauri::command]
fn decline_folder_ini(path: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    with_catalog_mut(&state, |cat| cat.decline_folder_ini_ingest(&path))
}

/// Albums (TASK-03-008): every album with its photo count and cover.
#[tauri::command]
fn list_albums(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<fotobase_catalog::AlbumInfo>, String> {
    with_catalog(&state, |cat| cat.list_albums())
}

/// Create an album (name + date required, place/description optional).
#[tauri::command]
fn create_album(
    name: String,
    date: String,
    place: Option<String>,
    description: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<fotobase_catalog::AlbumInfo, String> {
    with_catalog_sync(&state, |cat| {
        cat.create_album(&name, &date, place.as_deref(), description.as_deref())
    })
}

#[tauri::command]
fn update_album(
    album_id: i64,
    name: String,
    date: String,
    place: Option<String>,
    description: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<fotobase_catalog::AlbumInfo, String> {
    with_catalog_sync(&state, |cat| {
        cat.update_album(album_id, &name, &date, place.as_deref(), description.as_deref())
    })
}

/// Delete the album only; photos and originals are untouched.
#[tauri::command]
fn delete_album(album_id: i64, state: tauri::State<'_, AppState>) -> Result<(), String> {
    with_catalog_sync(&state, |cat| cat.delete_album(album_id))
}

/// Add photos to an album (`fb:Albums` in each sidecar). Returns the number
/// of photos that were not already members.
#[tauri::command]
fn add_to_album(
    album_id: i64,
    file_ids: Vec<i64>,
    state: tauri::State<'_, AppState>,
) -> Result<usize, String> {
    with_catalog_sync(&state, |cat| cat.add_to_album(album_id, &file_ids))
}

/// Remove photos from an album, soft-deleted (TASK-03-012): each removal
/// lands in the catalog trash and can be undone within the undo window.
/// Returns how many photos were members.
#[tauri::command]
fn remove_from_album(
    album_id: i64,
    file_ids: Vec<i64>,
    state: tauri::State<'_, AppState>,
) -> Result<usize, String> {
    with_catalog_sync(&state, |cat| cat.trash_remove_from_album(album_id, &file_ids))
}

/// Hide photos (TASK-03-009): `fb:Hidden` in each sidecar, library-only.
/// Soft-deleted into the trash for undo. Returns how many were hidden.
#[tauri::command]
fn hide_photos(file_ids: Vec<i64>, state: tauri::State<'_, AppState>) -> Result<usize, String> {
    with_catalog_sync(&state, |cat| cat.hide_photos(&file_ids))
}

#[tauri::command]
fn unhide_photos(file_ids: Vec<i64>, state: tauri::State<'_, AppState>) -> Result<usize, String> {
    with_catalog_sync(&state, |cat| cat.unhide_photos(&file_ids))
}

/// Hide a folder (TASK-03-009): the `.fotobase` flag; its photos and
/// subfolders inherit. Soft-deleted into the trash for undo.
#[tauri::command]
fn hide_folder(path: String, state: tauri::State<'_, AppState>) -> Result<bool, String> {
    with_catalog_sync(&state, |cat| cat.hide_folder(&path))
}

#[tauri::command]
fn unhide_folder(path: String, state: tauri::State<'_, AppState>) -> Result<bool, String> {
    with_catalog_sync(&state, |cat| cat.unhide_folder(&path))
}

/// The catalog trash (TASK-03-012): pending undoable actions, newest first.
#[tauri::command]
fn list_trash(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<fotobase_catalog::TrashEntry>, String> {
    with_catalog_sync(&state, |cat| cat.trash_list())
}

/// Undo one trash entry. An undone unlink re-adds the folder; the rescan
/// that brings its photos back runs on a blocking thread and announces
/// itself on `library-changed`.
#[tauri::command]
fn undo_trash(
    id: i64,
    state: tauri::State<'_, AppState>,
) -> Result<fotobase_catalog::UndoReport, String> {
    let report = with_catalog_sync(&state, |cat| cat.undo_trash(id))?;
    rescan_restored(&state, &report);
    Ok(report)
}

/// "Undo all" for one action type (`album_remove`, `hide` or `unlink`).
#[tauri::command]
fn undo_trash_all(
    action: fotobase_catalog::TrashAction,
    state: tauri::State<'_, AppState>,
) -> Result<fotobase_catalog::UndoReport, String> {
    let report = with_catalog_sync(&state, |cat| cat.undo_trash_all(action))?;
    rescan_restored(&state, &report);
    Ok(report)
}

/// Rescan the folders an undone unlink re-added (and re-arm their watchers)
/// off the command thread: a library folder can take a while to scan.
fn rescan_restored(state: &AppState, report: &fotobase_catalog::UndoReport) {
    if report.rescan.is_empty() {
        return;
    }
    let roots = report.rescan.clone();
    let app = state.app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        for root in roots {
            if let Err(err) = with_folder_manager(&state, |fm| fm.rescan_restored(&root)) {
                tracing::warn!(root, error = %err, "rescan after undoing an unlink failed");
            }
        }
        let _ = app.emit("library-changed", ());
    });
}

#[tauri::command]
fn set_album_cover(
    album_id: i64,
    file_id: i64,
    state: tauri::State<'_, AppState>,
) -> Result<fotobase_catalog::AlbumInfo, String> {
    with_catalog_sync(&state, |cat| cat.set_album_cover(album_id, file_id))
}

/// The album's photos as grid items (id + hash only, no pixels). A non-blank
/// `query` narrows them with the live search (filename, folder, caption).
#[tauri::command]
fn list_album_files(
    album_id: i64,
    query: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<fotobase_catalog::FileInfo>, String> {
    with_catalog(&state, |cat| match query.as_deref().map(str::trim) {
        Some(q) if !q.is_empty() => cat.search_album_files(album_id, q, usize::MAX),
        _ => cat.list_album_files(album_id, usize::MAX),
    })
}

/// Ids of the albums a photo is in (viewer / info pane).
#[tauri::command]
fn get_file_albums(file_id: i64, state: tauri::State<'_, AppState>) -> Result<Vec<i64>, String> {
    with_catalog(&state, |cat| cat.file_album_ids(file_id))
}

/// Tags (TASK-03-006): read a photo's tags from the catalog mirror.
#[tauri::command]
fn get_tags(file_id: i64, state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    with_catalog_sync(&state, |cat| tag_commands::tags(cat, file_id))
}

/// Add one tag (nested tags use the `|` path separator): write-through to
/// the sidecar, then the catalog mirror. Returns the updated list.
#[tauri::command]
fn add_tag(
    file_id: i64,
    tag: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    with_catalog_sync(&state, |cat| tag_commands::add(cat, file_id, &tag))
}

/// Remove one tag from a photo; returns the remaining list.
#[tauri::command]
fn remove_tag(
    file_id: i64,
    tag: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    with_catalog_sync(&state, |cat| tag_commands::remove(cat, file_id, &tag))
}

/// Rename a tag library-wide: every affected sidecar is rewritten (batch
/// rename, TASK-03-006). Returns the number of sidecars rewritten.
#[tauri::command]
fn rename_tag(old: String, new: String, state: tauri::State<'_, AppState>) -> Result<u32, String> {
    let n = with_catalog_sync(&state, |cat| tag_commands::rename(cat, &old, &new))?;
    Ok(n as u32)
}

/// Every tag in the library with usage counts (rename dialog source list).
#[tauri::command]
fn list_tags(state: tauri::State<'_, AppState>) -> Result<Vec<tag_commands::TagCount>, String> {
    with_catalog_sync(&state, |cat| tag_commands::all(cat))
}

/// Batch rename preview (TASK-03-011): the names a pattern would give the
/// photos, in selection order, plus any collisions. Touches nothing.
#[tauri::command]
fn preview_batch_rename(
    ids: Vec<i64>,
    spec: fotobase_scanner::batch::RenameSpec,
    policy: fotobase_scanner::batch::CollisionPolicy,
    state: tauri::State<'_, AppState>,
) -> Result<fotobase_scanner::batch::BatchPlan, String> {
    with_catalog_str(&state, |cat| batch_commands::preview_rename(cat, &ids, &spec, policy))
}

/// Date/time adjustment preview (TASK-03-019): per photo, the capture time
/// now and after `adjustment` — catalog data only, nothing is written.
#[tauri::command]
fn preview_datetime_adjustment(
    ids: Vec<i64>,
    adjustment: fotobase_catalog::TimeAdjustment,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<fotobase_catalog::TimePreview>, String> {
    with_catalog_str(&state, |cat| datetime_commands::preview(cat, &ids, &adjustment))
}

/// Shift / timezone fix / multi-camera merge (TASK-03-019, PLAN §6 P2a; a merge
/// is a shift by the clock difference the dialog computes): one
/// sidecar write per photo, catalog second, originals untouched. Heavy work
/// runs in the bounded job queue (§3 rule 4) on a blocking thread with its own
/// catalog connection; progress streams on the `datetime-progress` event.
#[tauri::command]
async fn adjust_datetime(
    ids: Vec<i64>,
    adjustment: fotobase_catalog::TimeAdjustment,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<fotobase_catalog::AdjustReport, String> {
    adjustment.validate().map_err(|e| e.to_string())?;
    let queue = state.datetime_jobs.clone();
    let job_app = app.clone();
    let catalog_path = state.catalog_path();
    let seq = DATETIME_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let handle = queue
        .try_enqueue(
            format!("datetime-{seq}"),
            fotobase_jobs::Priority::Background,
            fotobase_jobs::job(move |ctx| {
                let app = job_app.clone();
                let ids = ids.clone();
                let catalog_path = catalog_path.clone();
                let ctx = ctx.clone();
                async move {
                    let joined = tauri::async_runtime::spawn_blocking(move || {
                        let cancel = std::sync::atomic::AtomicBool::new(false);
                        datetime_commands::run(&catalog_path, &ids, &adjustment, &cancel, &|p| {
                            if ctx.is_cancelled() {
                                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                            let _ = app.emit(datetime_commands::DATETIME_EVENT, p);
                        })
                    })
                    .await;
                    match joined {
                        Ok(Ok(report)) => fotobase_jobs::Outcome::Done(report),
                        Ok(Err(e)) => fotobase_jobs::Outcome::Failed(e),
                        Err(e) => fotobase_jobs::Outcome::Failed(format!("datetime task: {e}")),
                    }
                }
            }),
        )
        .map_err(|e| format!("enqueue datetime adjustment: {e}"))?;
    let outcome = handle.result().await;
    let _ = app.emit("library-changed", ());
    match outcome.as_ref() {
        fotobase_jobs::Outcome::Done(report) => Ok(report.clone()),
        fotobase_jobs::Outcome::Cancelled => Err("date/time adjustment cancelled".into()),
        fotobase_jobs::Outcome::Failed(e) => Err(e.clone()),
    }
}

/// Portable export (TASK-03-023, PLAN §5 anti-lock-in): the picked user data
/// (albums / tags / recipes) as JSON or CSV into an existing folder. Read-only
/// for sidecars and catalog; runs in the bounded job queue (§3 rule 4) on a
/// blocking thread with its own catalog connection, so it works at any time,
/// even mid-scan.
#[tauri::command]
async fn export_user_data(
    dir: String,
    sections: fotobase_catalog::ExportSections,
    format: fotobase_catalog::ExportFormat,
    state: tauri::State<'_, AppState>,
) -> Result<fotobase_catalog::ExportReport, String> {
    let queue = state.exports.clone();
    let catalog_path = state.catalog_path();
    let seq = EXPORT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let handle = queue
        .try_enqueue(
            format!("export-{seq}"),
            fotobase_jobs::Priority::Background,
            fotobase_jobs::job(move |_ctx| {
                let (dir, catalog_path) = (dir.clone(), catalog_path.clone());
                async move {
                    let joined = tauri::async_runtime::spawn_blocking(move || {
                        let cat = Catalog::open(&catalog_path).map_err(|e| e.to_string())?;
                        cat.export_user_data(std::path::Path::new(&dir), sections, format)
                            .map_err(|e| e.to_string())
                    })
                    .await;
                    match joined {
                        Ok(Ok(report)) => fotobase_jobs::Outcome::Done(report),
                        Ok(Err(e)) => fotobase_jobs::Outcome::Failed(e),
                        Err(e) => fotobase_jobs::Outcome::Failed(format!("export task: {e}")),
                    }
                }
            }),
        )
        .map_err(|e| format!("enqueue export: {e}"))?;
    match handle.result().await.as_ref() {
        fotobase_jobs::Outcome::Done(report) => Ok(report.clone()),
        fotobase_jobs::Outcome::Cancelled => Err("export cancelled".into()),
        fotobase_jobs::Outcome::Failed(e) => Err(e.clone()),
    }
}

/// Batch rename: real file renames, sidecars and catalog rows following.
/// All-or-nothing; returns the applied moves (the undo window's input).
#[tauri::command]
async fn batch_rename(
    ids: Vec<i64>,
    spec: fotobase_scanner::batch::RenameSpec,
    policy: fotobase_scanner::batch::CollisionPolicy,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<fotobase_scanner::batch::PlannedMove>, String> {
    let moves = with_catalog_str(&state, |cat| batch_commands::rename(cat, &ids, &spec, policy))?;
    let _ = app.emit("library-changed", ());
    Ok(moves)
}

/// Batch move preview: where the photos would land in `dest`.
#[tauri::command]
fn preview_batch_move(
    ids: Vec<i64>,
    dest: String,
    policy: fotobase_scanner::batch::CollisionPolicy,
    state: tauri::State<'_, AppState>,
) -> Result<fotobase_scanner::batch::BatchPlan, String> {
    with_catalog_str(&state, |cat| batch_commands::preview_move(cat, &ids, &dest, policy))
}

/// Batch move into a cataloged folder: real file moves, sidecars and
/// catalog rows following. All-or-nothing, like [`batch_rename`].
#[tauri::command]
async fn batch_move(
    ids: Vec<i64>,
    dest: String,
    policy: fotobase_scanner::batch::CollisionPolicy,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<fotobase_scanner::batch::PlannedMove>, String> {
    let moves =
        with_catalog_str(&state, |cat| batch_commands::move_photos(cat, &ids, &dest, policy))?;
    let _ = app.emit("library-changed", ());
    Ok(moves)
}

/// [`with_catalog_mut`] for helpers that already report `String` errors.
fn with_catalog_str<T>(
    state: &AppState,
    f: impl FnOnce(&mut Catalog) -> Result<T, String>,
) -> Result<T, String> {
    let mut guard = state.catalog.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))?;
    if guard.is_none() {
        *guard = Some(open_shell_catalog(state)?);
    }
    f(guard.as_mut().expect("catalog just inserted"))
}

/// [`with_catalog_mut`] for sync-engine errors (tags work through the sync
/// engine, whose error type is richer than the plain catalog's).
fn with_catalog_sync<T>(
    state: &AppState,
    f: impl FnOnce(&mut Catalog) -> Result<T, fotobase_catalog::SyncError>,
) -> Result<T, String> {
    let mut guard = state.catalog.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))?;
    if guard.is_none() {
        *guard = Some(open_shell_catalog(state)?);
    }
    f(guard.as_mut().expect("catalog just inserted")).map_err(|e| e.to_string())
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

/// Player payload for one video (TASK-02-007): path + ffprobe result. The
/// viewer calls this for the item it is about to display; `None` means it is
/// an image (or unknown) and the normal `preview://` path applies. No media
/// bytes — the path feeds the asset-protocol URL, `stream://` takes the id.
#[tauri::command]
fn video_info(
    file_id: i64,
    state: tauri::State<'_, AppState>,
) -> Result<Option<fotobase_catalog::VideoInfo>, String> {
    with_catalog(&state, |cat| cat.video_info(file_id))
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

// --- stream:// transcode fallback (TASK-02-006, PLAN §2 Video strategy) -----

fn stream_response(reply: stream::StreamReply) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(reply.status)
        .header("content-type", reply.content_type)
        .header("access-control-allow-origin", "*")
        // A transcode stream is per-request (the player re-requests at the
        // current position to continue or seek); it must never be cached.
        .header("cache-control", "no-store")
        .body(reply.body)
        .expect("static stream response parts are valid")
}

/// Resolve a `stream://<file-id>` request against the catalog and serve the
/// transcode. Unknown ids and non-video files get a 404 — the player routes
/// only video here, so anything else is a stale/malformed request.
fn stream_reply_for(app: &tauri::AppHandle, req: stream::StreamRequest) -> stream::StreamReply {
    let state = app.state::<AppState>();
    let info = with_catalog(&state, |cat| cat.photo_info(req.file_id)).ok().flatten();
    let Some(info) = info else {
        return stream::StreamReply { status: 404, content_type: "text/plain", body: Vec::new() };
    };
    if info.media_type != "video" {
        return stream::StreamReply { status: 404, content_type: "text/plain", body: Vec::new() };
    }
    // The target intermediate is chosen from the WebView's probe matrix
    // (H.264/mp4 preferred, VP9/webm fallback); an unprobed matrix defaults
    // to the H.264 best effort inside `choose_target`.
    let support = state.codec_support.lock().ok().and_then(|g| *g);
    let job = stream::StreamJob {
        bin: fotobase_video::ffmpeg_bin(),
        src: info.path.into(),
        target: stream::choose_target(support.as_ref()),
        start_secs: req.start_secs,
        file_id: req.file_id,
    };
    stream::serve(&job, &state.streams)
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
    let cache_dir = state.cache_dir();
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

/// Tray quit, exposed over IPC for the e2e suite (TASK-03-025): WebDriver
/// cannot click the OS tray, so the spec drives the same [`tray::quit`]
/// code path the menu item runs.
#[tauri::command]
fn quit_from_tray(app: tauri::AppHandle) {
    tray::quit(&app);
}

/// Close-to-tray over IPC for the e2e suite (TASK-03-025): runs the same
/// interception the titlebar close button triggers, minus the `hide()` —
/// WebKitWebDriver cannot execute against a hidden view, so under the driver
/// the intercepted window stays mapped. (The JS `close()` API is unusable
/// here too: on Linux/WebKitGTK it destroys the webview even when the close
/// request is prevented — tauri-apps/tauri#8435.)
#[tauri::command]
fn close_main_window(app: tauri::AppHandle) {
    if !tray::close_intercepted(&app) {
        // Interception declined (close_to_tray off): close for real, the
        // same outcome the un-intercepted titlebar close would have.
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.destroy();
        }
    }
}

// --- Codec capability (TASK-02-005, PLAN §10) --------------------------------

/// The frontend WebView probes its own decoders once at startup and reports
/// the matrix here; the probe never blocks startup (functional spec) — this
/// command is fire-and-forget from the frontend's boot sequence.
#[tauri::command]
fn report_codec_support(
    state: tauri::State<'_, AppState>,
    support: codec::CodecSupport,
) -> Result<(), String> {
    tracing::debug!("{}", support.log_line());
    state
        .codec_support
        .lock()
        .map_err(|e| format!("codec-support mutex poisoned: {e}"))?
        .replace(support);
    Ok(())
}

/// Diagnostics surface for the cached matrix (`None` until the frontend
/// has probed).
#[tauri::command]
fn get_codec_support(
    state: tauri::State<'_, AppState>,
) -> Result<Option<codec::CodecSupport>, String> {
    Ok(*state.codec_support.lock().map_err(|e| format!("codec-support mutex poisoned: {e}"))?)
}

/// Player stop/destroy (TASK-02-006): cancel the in-flight transcode for a
/// file, killing and reaping the ffmpeg child — no orphan processes. `true`
/// when something was actually cancelled.
#[tauri::command]
fn stop_stream(state: tauri::State<'_, AppState>, file_id: i64) -> Result<bool, String> {
    Ok(state.streams.kill(file_id))
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
    {
        let mut guard =
            state.settings.lock().map_err(|e| format!("settings mutex poisoned: {e}"))?;
        *guard = new.clone();
    }
    // "Show hidden" and the undo window take effect on the live connection
    // (settings lock released first: the catalog opener takes it too).
    let mut guard = state.catalog.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))?;
    if let Some(cat) = guard.as_mut() {
        trash_commands::apply_settings(cat, &new)?;
    }
    Ok(())
}

/// "Clear cache files": drain the render queue, then empty the thumbnail
/// cache. Heavy work, so it runs as an async command; the library and the
/// catalog are never touched.
#[tauri::command]
async fn clear_cache(state: tauri::State<'_, AppState>) -> Result<u64, String> {
    // Clone before the await: a borrow derived from `State` must not be
    // held across an await point (the command future has to stay Send).
    let cache_dir = state.cache_dir();
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
    let job_catalog_path = state.catalog_path();
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
/// One row of the switcher UI: name, directory, and whether it is active.
#[derive(Debug, Serialize)]
pub struct CatalogSummary {
    pub name: String,
    pub dir: String,
    pub current: bool,
}

/// List registered catalogs for the Options switcher (TASK-03-024).
#[tauri::command]
fn list_catalogs(state: tauri::State<'_, AppState>) -> Result<Vec<CatalogSummary>, String> {
    let registry = state.registry.lock().map_err(|e| format!("registry mutex poisoned: {e}"))?;
    let current = state.catalog_path();
    Ok(registry
        .catalogs
        .iter()
        .map(|c| CatalogSummary {
            name: c.name.clone(),
            dir: c.dir.to_string_lossy().into_owned(),
            current: catalog_db_path(&c.dir) == current,
        })
        .collect())
}

/// Create a new catalog (empty DB + thumbnail cache under its own
/// directory) and switch to it (TASK-03-024 functional spec: "create
/// additional catalogs from options").
#[tauri::command]
async fn create_catalog(
    name: String,
    dir: Option<String>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let dir = match dir {
        Some(d) if !d.trim().is_empty() => PathBuf::from(d),
        // Default location: a sibling directory under the data dir.
        _ => state.data_dir.join("catalogs").join(catalog_slug(&name)),
    };
    let entry = {
        let mut registry =
            state.registry.lock().map_err(|e| format!("registry mutex poisoned: {e}"))?;
        registry.create(&name, &dir).map_err(|e| e.to_string())?.clone()
    };
    registry_save(&state)?;
    switch_to_catalog(&state, &app, &entry).await
}

/// Switch the active catalog (TASK-03-024): stop watchers, flush the job
/// queue, close the DB, reopen the target — no app restart beyond the
/// frontend reload the `catalog-switched` event triggers.
#[tauri::command]
async fn switch_catalog(
    name: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let entry = {
        let registry =
            state.registry.lock().map_err(|e| format!("registry mutex poisoned: {e}"))?;
        registry.get(&name).ok_or_else(|| format!("no catalog named {name:?}"))?.clone()
    };
    switch_to_catalog(&state, &app, &entry).await
}

/// Used by the tray menu, which identifies catalogs by name too.
pub async fn switch_catalog_by_name(app: &tauri::AppHandle, name: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let entry = {
        let registry =
            state.registry.lock().map_err(|e| format!("registry mutex poisoned: {e}"))?;
        registry.get(name).ok_or_else(|| format!("no catalog named {name:?}"))?.clone()
    };
    switch_to_catalog(&state, app, &entry).await
}

/// Filesystem slug for a default catalog directory ("My Work" → "my-work").
fn catalog_slug(name: &str) -> String {
    let slug: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "catalog".to_string()
    } else {
        slug
    }
}

/// Persist the registry after a create/switch (best-effort: a failed save
/// only costs the last-used memory, never catalog data).
fn registry_save(state: &AppState) -> Result<(), String> {
    state
        .registry
        .lock()
        .map_err(|e| format!("registry mutex poisoned: {e}"))?
        .save(&state.data_dir)
        .map_err(|e| format!("save registry: {e}"))
}

/// The switch lifecycle (technical spec): stop watchers, flush the job
/// queue, close the DB (WAL checkpoint), reopen the target catalog, restart
/// watchers. Reuses the tray-icon lifecycle from 03-025: the tray menu is
/// rebuilt so the checkmark follows the active catalog.
async fn switch_to_catalog(
    state: &tauri::State<'_, AppState>,
    app: &tauri::AppHandle,
    entry: &catalog_registry::CatalogEntry,
) -> Result<(), String> {
    let target_db = catalog_db_path(&entry.dir);
    let target_cache = catalog_cache_dir(&entry.dir);
    if state.catalog_path() == target_db {
        return Ok(());
    }

    // 1. Stop the watcher: dropping the FolderManager stops its threads and
    //    closes its own catalog connection — no watcher keeps firing into a
    //    catalog we are about to close.
    if let Some(fm) =
        state.folders.lock().map_err(|e| format!("folder manager mutex poisoned: {e}"))?.take()
    {
        fm.stop();
    }

    // 2. Flush the job queue (PLAN §3 rule 4): drain waits for queued and
    //    running jobs. Jobs captured the old catalog path at enqueue time,
    //    so they finish writing the OLD catalog before we close it — no
    //    half-written work, no cross-catalog writes.
    state.thumbs.drain().await;
    state.rebuilds.drain().await;
    state.imports.drain().await;
    state.datetime_jobs.drain().await;
    state.exports.drain().await;

    // 3. Close the DB: dropping the connection closes it; SQLite
    //    checkpoints the WAL when the last connection closes. The next
    //    command lazily reopens against the new path.
    *state.catalog.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))? = None;

    // 4. Reopen target: swap paths, make sure its cache dir exists, and
    //    restart the watcher lazily (`with_folder_manager` re-creates the
    //    FolderManager against the new catalog on its next use).
    std::fs::create_dir_all(&target_cache).map_err(|e| format!("create cache dir: {e}"))?;
    *state.catalog_path.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))? = target_db;
    *state.cache_dir.lock().map_err(|e| format!("cache mutex poisoned: {e}"))? = target_cache;

    // 5. Record last-used so the next launch opens this catalog.
    {
        let mut registry =
            state.registry.lock().map_err(|e| format!("registry mutex poisoned: {e}"))?;
        registry.switch(&entry.name).map_err(|e| e.to_string())?;
    }
    registry_save(state)?;

    // 6. Tell the frontend (it reloads — the grid, search index and viewer
    //    state all belong to the old catalog) and rebuild the tray menu so
    //    its checkmark follows.
    let _ = app.emit("catalog-switched", &entry.name);
    let _ = app.emit("library-changed", ());
    tray::refresh_catalogs(app);
    tracing::info!(catalog = %entry.name, "switched catalog");
    Ok(())
}

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
                Some(req) => thumb::resolve_thumb(req, &state.cache_dir()),
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
                            &state.cache_dir(),
                            &state.thumbs,
                            state.display_profile.lock().ok().and_then(|g| g.clone()),
                            // TASK-02-004: videos extract via the shipped
                            // ffmpeg sidecar at background priority.
                            Some(state.video_extractor.clone()),
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
        // TASK-02-006 (PLAN §2 Video strategy): on-the-fly transcode for
        // codecs the WebView cannot decode. Asynchronous handler: the
        // transcode runs on its own thread (bounded by byte cap + first-
        // bytes budget), the catalog lookup happens there too, and the
        // response never outlives its ffmpeg child.
        .register_asynchronous_uri_scheme_protocol(
            stream::STREAM_PROTOCOL,
            |ctx, request, responder| {
                let app = ctx.app_handle().clone();
                let req = stream::request_from_uri(request.uri());
                std::thread::spawn(move || {
                    let reply = match req {
                        Some(req) => stream_reply_for(&app, req),
                        None => stream::StreamReply {
                            status: 404,
                            content_type: "text/plain",
                            body: Vec::new(),
                        },
                    };
                    responder.respond(stream_response(reply));
                });
            },
        )
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
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
            let datetime_jobs = tauri::async_runtime::block_on(async {
                fotobase_jobs::Queue::new(DATETIME_QUEUE_CAPACITY)
            });
            let exports = tauri::async_runtime::block_on(async {
                fotobase_jobs::Queue::new(EXPORT_QUEUE_CAPACITY)
            });
            let persisted = settings::Settings::load(&data_dir);
            // Catalog registry (TASK-03-024): which catalog opens at launch
            // (the last-used one, functional spec). The default entry keeps
            // the legacy layout — DB + thumbnail cache directly in the data
            // dir — so existing installs need no migration.
            let registry = catalog_registry::Registry::load(&data_dir);
            let entry = registry.entry_for_launch().clone();
            let catalog_path = catalog_db_path(&entry.dir);
            let cache_dir = catalog_cache_dir(&entry.dir);
            std::fs::create_dir_all(&cache_dir)?;
            tracing::info!(catalog = %entry.name, "opening catalog");
            // OS-opened files (TASK-01-025): whatever the OS passed on the
            // command line that is an existing supported image is parked
            // for the frontend to pick up and route into the viewer.
            let pending_open: Vec<String> =
                associations::collect_open_paths(&std::env::args().skip(1).collect::<Vec<_>>());
            app.manage(AppState {
                app_handle: app.handle().clone(),
                data_dir: data_dir.clone(),
                catalog_path: Mutex::new(catalog_path),
                catalog: Mutex::new(None),
                cache_dir: Mutex::new(cache_dir),
                registry: Mutex::new(registry),
                thumbs,
                rebuilds,
                imports,
                datetime_jobs,
                exports,
                import_cancel: Mutex::new(None),
                settings: Mutex::new(persisted),
                pending_update: Mutex::new(None),
                folders: Mutex::new(None),
                pending_open: Mutex::new(pending_open),
                display_profile: Mutex::new(initial_display_profile()),
                video_extractor: std::sync::Arc::new(fotobase_video::Ffmpeg::default()),
                codec_support: Mutex::new(None),
                streams: stream::StreamRegistry::new(),
            });
            // Display-profile watcher (TASK-02-001): re-read the profile
            // every [`display_profile::POLL_INTERVAL`]; on a fingerprint
            // change swap it into the shared state and tell the WebView, so
            // the grid re-requests every thumbnail and lands in the new
            // profile-tagged cache entries — no app restart needed.
            let watcher_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut last = fotobase_render::color::profile_fingerprint(
                    initial_display_profile().as_deref().unwrap_or_default(),
                );
                loop {
                    tokio::time::sleep(display_profile::POLL_INTERVAL).await;
                    let profile = initial_display_profile();
                    let fp = fotobase_render::color::profile_fingerprint(
                        profile.as_deref().unwrap_or_default(),
                    );
                    if fp == last {
                        continue;
                    }
                    if let Some(state) = watcher_app.try_state::<AppState>() {
                        if let Ok(mut guard) = state.display_profile.lock() {
                            *guard = profile;
                        }
                        tracing::info!("display profile changed to {fp}; re-rendering previews");
                        let _ = watcher_app.emit("display-profile-changed", fp.clone());
                    }
                    last = fp;
                }
            });
            // stream:// janitor (TASK-02-006): the idle-timeout backstop —
            // a transcode that outlived its request (hung child, aborted
            // request) is killed and reaped even if nobody called
            // `stop_stream`.
            let janitor_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(stream::JANITOR_INTERVAL).await;
                    if let Some(state) = janitor_app.try_state::<AppState>() {
                        let killed = state.streams.kill_idle(stream::IDLE_TIMEOUT);
                        if killed > 0 {
                            tracing::info!("stream janitor killed {killed} idle transcode(s)");
                        }
                    }
                }
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
                perf_probe::spawn_completion_watcher(state.catalog_path(), cfg.expected);
            }
            // Tray icon (TASK-03-025, PLAN §6 P2a): shows and controls
            // background watching; the window sits hidden under it when
            // closed. Failure is non-fatal — the app runs without a tray.
            if let Err(e) = tray::build_tray(app.handle()) {
                tracing::warn!("tray icon unavailable: {e}");
            }
            tray::spawn_status_updater(app.handle().clone());
            Ok(())
        })
        // Close-to-tray (TASK-03-025): the close button hides the window
        // and watching continues; quitting happens from the tray menu
        // (fully exits) or when the setting is off.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if tray::handle_close_request(window.app_handle()) {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            quit_from_tray,
            close_main_window,
            report_codec_support,
            get_codec_support,
            stop_stream,
            search_files,
            list_files,
            query_files,
            set_rating,
            get_caption,
            set_caption,
            edit_photo_metadata,
            stack_photos,
            set_stack_top,
            unstack,
            list_stack,
            stack_suggestions,
            folder_ini_pending_folders,
            ingest_folder_ini,
            decline_folder_ini,
            list_albums,
            create_album,
            update_album,
            delete_album,
            add_to_album,
            remove_from_album,
            hide_photos,
            unhide_photos,
            hide_folder,
            unhide_folder,
            list_trash,
            undo_trash,
            undo_trash_all,
            set_album_cover,
            list_album_files,
            get_file_albums,
            get_tags,
            add_tag,
            remove_tag,
            rename_tag,
            list_tags,
            preview_datetime_adjustment,
            adjust_datetime,
            export_user_data,
            preview_batch_rename,
            batch_rename,
            preview_batch_move,
            batch_move,
            photo_info,
            video_info,
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
            list_catalogs,
            create_catalog,
            switch_catalog,
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
