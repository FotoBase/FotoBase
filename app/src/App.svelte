<script lang="ts">
  // Skeleton shell (library grid arrives with 01-013+). This carries the
  // first wiring of the Options dialog (TASK-01-022): settings load at
  // startup, the dialog persists every change, and the update-mode policy
  // runs once on startup.
  import { untrack } from "svelte";
  import AlbumDialog from "./lib/AlbumDialog.svelte";
  import AlbumList from "./lib/AlbumList.svelte";
  import BatchDialog from "./lib/BatchDialog.svelte";
  import CanvasThumbnailGrid from "./lib/CanvasThumbnailGrid.svelte";
  import ContextMenu from "./lib/ContextMenu.svelte";
  import DateTimeDialog from "./lib/DateTimeDialog.svelte";
  import FolderManagerDialog from "./lib/FolderManagerDialog.svelte";
  import FolderTree from "./lib/FolderTree.svelte";
  import ImportDialog from "./lib/ImportDialog.svelte";
  import OptionsDialog from "./lib/OptionsDialog.svelte";
  import PhotoTray from "./lib/PhotoTray.svelte";
  import FolderIniDialog from "./lib/FolderIniDialog.svelte";
  import { onFolderIniOffered, folderIniPendingFolders } from "./lib/folder-ini";
  import QuickTags from "./lib/QuickTags.svelte";
  import StackPicker from "./lib/StackPicker.svelte";
  import StackSuggestionsDialog from "./lib/StackSuggestionsDialog.svelte";
  import SearchBar from "./lib/SearchBar.svelte";
  import FilterBar from "./lib/FilterBar.svelte";
  import { cutoffDate } from "./lib/filters";
  import ThumbnailGrid from "./lib/ThumbnailGrid.svelte";
  import TrashDialog from "./lib/TrashDialog.svelte";
  import Viewer from "./lib/Viewer.svelte";
  import EditView from "./lib/EditView.svelte";
  import { deleteFromDisk, exitApp, refreshFolderThumbnails, revealInDisk } from "./lib/file-menu";
  import {
    addToAlbum,
    deleteAlbum,
    listAlbums,
    removeFromAlbum,
    setAlbumCover,
    targetIds,
    type Album,
  } from "./lib/albums";
  import { reportCodecSupport } from "./lib/codec-probe";
  import { removeFolder } from "./lib/folder-manager";
  import type { FolderInfo, MenuItem } from "./lib/folder-tree";
  import { debounce, throttle, type GridItem } from "./lib/grid";
  import { gridState, pickRenderer } from "./lib/grid-state.svelte.js";
  import { setLocale, t } from "./lib/i18n.svelte.js";
  import { loadPhotoInfo } from "./lib/info-pane";
  import { queryLibraryItems } from "./lib/search";
  import { findFileByPath, loadSettings, saveSettings, takeOpenFiles, type Settings } from "./lib/settings";
  import { captionModeFromSettings, settingsWithCaptionMode } from "./lib/captions";
  import {
    DEFAULT_FOLDER_VIEW,
    folderViewFromSettings,
    settingsWithFolderView,
    type FolderViewOptions,
  } from "./lib/folder-sort";
  import { shortcuts, type ShortcutHandlers } from "./lib/shortcuts";
  import { canStack, stackPhotos, unstack } from "./lib/stacks";
  import { applyTheme, resolveTheme, systemPrefersDark, watchSystemTheme } from "./lib/theme";
  import { actionIds, tray, type TrayPhoto } from "./lib/tray.svelte.js";
  import { hideFolder, hidePhotos, unhideFolder, unhidePhotos } from "./lib/trash";
  import { runUpdateFlow } from "./lib/updates";

  let pong = $state<string | null>(null);
  let optionsOpen = $state(false);
  let folderManagerOpen = $state(false);
  let importOpen = $state(false);
  let trashOpen = $state(false);
  // Folder ini migration offer (TASK-03-022): folders a scan found an ini
  // in; the dialog opens whenever one is waiting for an answer.
  let folderIniOpen = $state(false);
  let folderIniFolders = $state<string[]>([]);
  // Stacking (TASK-03-021): suggested RAW+JPEG pairs / edited versions, and
  // the picker a stack badge opens.
  let stackSuggestionsOpen = $state(false);
  let stackPicker = $state<{ open: boolean; stackId: string; x: number; y: number }>({
    open: false,
    stackId: "",
    x: 0,
    y: 0,
  });
  let settings = $state<Settings | null>(null);
  let photos = $state<GridItem[]>([]);
  let folders = $state<FolderInfo[]>([]);
  // Folder sort + flat/tree layout (TASK-03-016), persisted in settings.
  let folderView = $state<FolderViewOptions>({ ...DEFAULT_FOLDER_VIEW });
  let selectedFolders = $state<number[]>([]);
  // Albums (TASK-03-008): the sidebar list, the album whose photos the grid
  // shows (null = the folder/search view), and the menus + dialog around it.
  let albums = $state<Album[]>([]);
  let selectedAlbum = $state<number | null>(null);
  let albumDialog = $state<{ open: boolean; album: Album | null }>({ open: false, album: null });
  let albumMenu = $state<{ open: boolean; x: number; y: number; album: Album | null }>({
    open: false,
    x: 0,
    y: 0,
    album: null,
  });
  // "Add to album" picker: opened from the photo menu, lists the albums.
  let albumPicker = $state<{ open: boolean; x: number; y: number; ids: number[] }>({
    open: false,
    x: 0,
    y: 0,
    ids: [],
  });
  // Batch rename / move (TASK-03-011): opened from the tray, acts on its photos.
  let batchDialog = $state<{ open: boolean; mode: "rename" | "move"; ids: number[] }>({
    open: false,
    mode: "rename",
    ids: [],
  });
  // Date/time adjustment (TASK-03-019): opened from the photo menu, acts on
  // the right-clicked photo or the whole selection it belongs to.
  let dateTimeDialog = $state<{ open: boolean; ids: number[] }>({ open: false, ids: [] });
  let folderMenu = $state<{ open: boolean; x: number; y: number; folder: FolderInfo | null }>({
    open: false,
    x: 0,
    y: 0,
    folder: null,
  });
  // Per-photo context menu (TASK-01-021): the right-clicked grid cell.
  let photoMenu = $state<{ open: boolean; x: number; y: number; item: GridItem | null }>({
    open: false,
    x: 0,
    y: 0,
    item: null,
  });

  // Single-photo viewer (TASK-01-019): opened from the grid over the current
  // folder/search selection. `slideshow` is set by the context menu's
  // "View slideshow" entry; keyboard control lives inside the viewer.
  let viewer = $state<{ open: boolean; index: number; slideshow: boolean }>({
    open: false,
    index: 0,
    slideshow: false,
  });

  // Single-photo edit view (TASK-04-010): opened from the viewer's Edit
  // button; replaces the viewer while open (both listen on window keys, so
  // only one may be mounted) and closes back into it.
  let edit = $state<{ open: boolean; index: number }>({ open: false, index: 0 });

  function openEdit(item: GridItem): void {
    const index = photos.findIndex((p) => p.id === item.id);
    edit = { open: true, index: index < 0 ? 0 : index };
  }

  function openViewer(item: GridItem, slideshow = false): void {
    const index = photos.findIndex((p) => p.id === item.id);
    viewer = { open: true, index: index < 0 ? 0 : index, slideshow };
  }

  function openSlideshow(): void {
    // No selection: start from the first photo of the current view.
    const first = photos[0];
    if (first) openViewer(first, true);
  }

  // Theme (TASK-01-024): resolve the setting against the OS preference and
  // publish it as a data-theme attribute the stylesheet keys off. While the
  // setting is "system", keep following the OS live. The effect re-runs
  // when the setting changes and tears the listener down when it doesn't.
  $effect(() => {
    const setting = settings?.theme;
    if (!setting) return;
    applyTheme(resolveTheme(setting, systemPrefersDark()));
    if (setting !== "system") return;
    return watchSystemTheme((dark) => applyTheme(resolveTheme(setting, dark)));
  });

  // Global keyboard shortcuts (TASK-01-024, docs/help/shortcuts.md). The
  // component-local keys (grid arrows/Enter, viewer navigation) live in
  // their own components; these are the shell-level ones.
  let searchEl: { focus(): void; clear(): void } | null = $state(null);
  const globalShortcuts: ShortcutHandlers = {
    "search-focus": () => searchEl?.focus(),
    "tray-clear": () => {
      tray.clearUnheld();
    },
    exit: () => {
      void exitApp().catch(() => {
        // The backend tears down mid-response on a clean exit — not an error.
      });
    },
  };

  async function ping(): Promise<void> {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      pong = await invoke<string>("ping");
    } catch {
      pong = t("app.backendMissing");
    }
  }

  // Scrolling reprioritizes render jobs (01-013): the grid reports the
  // hashes on screen (already debounced), the backend bumps them to
  // visible-first priority (PLAN §3 rule 4). Best-effort — a missing or
  // failing backend must never break the grid.
  const prioritizeVisible = debounce((hashes: string[]) => {
    void (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("prioritize_visible", { hashes });
      } catch {
        // Browser dev / backend down: nothing to prioritize.
      }
    })();
  }, 150);

  // The one grid query (TASK-03-014): folder selection / album view, live
  // search and the filter buttons + date slider AND-combine in the backend.
  // The filter state is read before the first `await`, so calling this from
  // an `$effect` tracks every input. The shell's `photos` feed both the grid
  // and the viewer, so the two always show the same filtered set.
  let loadSeq = 0;
  function loadPhotos(sel: number[], album: number | null): Promise<GridItem[]> {
    return queryLibraryItems({
      // An album view takes the place of the folder filter.
      folders: album !== null ? [] : sel,
      album,
      query: gridState.searchQuery,
      starred: gridState.starredOnly,
      videosOnly: gridState.videosOnly,
      takenSince: cutoffDate(gridState.dateStep),
      sort: gridState.sort,
    });
  }

  // Library refresh (TASK-01-016): the startup load and every Folder
  // Manager change (mode applied / folder removed) re-read folders + files,
  // so the grid mirrors the catalog immediately.
  async function refreshLibrary(): Promise<void> {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      photos = await loadPhotos(selectedFolders, selectedAlbum);
      folders = await invoke<FolderInfo[]>("list_folders");
      albums = await listAlbums();
    } catch {
      // Browser dev: no backend, empty grid and tree.
      photos = [];
      folders = [];
      albums = [];
    }
  }

  // Hide/unhide/undo changed what the library lists: re-run the current
  // (folder / album / search) view, not the whole-library list, then refresh
  // the tree and the album counts.
  async function refreshView(): Promise<void> {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      photos = await loadPhotos(selectedFolders, selectedAlbum);
      folders = await invoke<FolderInfo[]>("list_folders");
      albums = await listAlbums();
      const visible = new Set(folders.map((f) => f.id));
      selectedFolders = selectedFolders.filter((id) => visible.has(id));
    } catch {
      // Backend down: keep what is shown.
    }
  }

  // Album membership changed (add/remove/delete/cover): refresh the sidebar
  // counts and, in an album view, the grid.
  async function refreshAlbums(): Promise<void> {
    try {
      albums = await listAlbums();
      if (selectedAlbum !== null && !albums.some((a) => a.id === selectedAlbum)) {
        selectedAlbum = null;
      }
      if (selectedAlbum !== null) {
        photos = await loadPhotos(selectedFolders, selectedAlbum);
      }
    } catch {
      // Browser dev / backend down: keep what is shown.
    }
  }

  // Caption mode (TASK-03-015): the grid toolbar flips the shared store; the
  // choice is persisted as the `caption_mode` setting. Settings are read
  // untracked so only a mode change (not an Options edit) triggers a save.
  $effect(() => {
    const mode = gridState.captionMode;
    const next = untrack(() => (settings ? settingsWithCaptionMode(settings, mode) : null));
    if (!next) return;
    settings = next;
    void saveSettings(next).catch(() => {
      // Browser dev / backend down: the choice still holds for the session.
    });
  });

  // Folder view options (TASK-03-016): saved on change, like the caption mode.
  function changeFolderView(next: FolderViewOptions): void {
    folderView = next;
    const updated = settings ? settingsWithFolderView(settings, next) : null;
    if (!updated) return;
    settings = updated;
    void saveSettings(updated).catch(() => {
      // Browser dev / backend down: the choice still holds for the session.
    });
  }

  $effect(() => {
    // Startup: load persisted settings, apply the language, then honor the
    // update policy (auto installs, check-but-ask prompts, off does
    // nothing — the backend gate enforces that too).
    void (async () => {
      const s = await loadSettings();
      settings = s;
      gridState.captionMode = captionModeFromSettings(s);
      folderView = folderViewFromSettings(s);
      setLocale(s.language);
      // Codec probe (TASK-02-005): fire-and-forget so it never blocks
      // startup (functional spec); the cached matrix drives the HTML5 vs
      // `stream://` playback decision (02-006).
      void reportCodecSupport();
      await runUpdateFlow(s.update_mode);
      await refreshLibrary();
      await openOsFiles();
    })();
  });

  // OS-opened files (TASK-01-025, PLAN §2M): when the user opens an image
  // with Fotobase from the file manager, the backend parks the path at
  // startup (argv on Windows/Linux, LaunchServices `RunEvent::Opened` on
  // macOS); here it is resolved against the catalog and the viewer opens
  // on that row. Uncataloged paths are ignored — the file first needs to
  // live in a watched folder. Best-effort: a missing backend (browser dev)
  // must never break startup.
  async function handleOpenPaths(paths: string[]): Promise<void> {
    for (const path of paths) {
      try {
        const match = await findFileByPath(path);
        if (!match) continue;
        const index = photos.findIndex((p) => p.id === match.id);
        if (index >= 0) {
          viewer = { open: true, index, slideshow: false };
          break;
        }
      } catch {
        // Backend down mid-lookup: leave the viewer alone.
      }
    }
  }

  async function openOsFiles(): Promise<void> {
    try {
      await handleOpenPaths(await takeOpenFiles());
    } catch {
      // Browser dev / backend down: nothing to open.
    }
  }

  // Files opened with Fotobase while it is already running (macOS
  // open-with): the backend announces them on `open-file`.
  $effect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen<string[]>("open-file", (e) => {
          void handleOpenPaths(e.payload);
        });
      } catch {
        // Browser dev: no backend, no events.
      }
    })();
    return () => unlisten?.();
  });

  // Cache-miss re-bust (PLAN §3 rule 3): a cold-start `thumb://` miss serves
  // the placeholder and queues a render; when the render lands the backend
  // emits `thumb-rendered`. Bumping the shared store's per-hash version
  // changes the URI every consumer builds, so the WebView re-requests and
  // the placeholder is replaced. Without this the first-ever view of a
  // library stays gray until something else re-renders the cell.
  $effect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen<{ hash: string; scale: number }>("thumb-rendered", (e) => {
          gridState.bumpThumbVersion(e.payload.hash);
        });
      } catch {
        // Browser dev: no backend, no events.
      }
    })();
    return () => unlisten?.();
  });

  // Display-profile switch (TASK-02-001): the backend re-reads the display
  // ICC profile every few seconds and emits `display-profile-changed` when
  // it changes. Bumping every hash's version re-requests each on-screen
  // thumbnail, which now resolves into the new profile's cache entries —
  // the whole grid re-colors without an app restart.
  $effect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen("display-profile-changed", () => {
          gridState.bumpAllThumbVersions();
        });
      } catch {
        // Browser dev: no backend, no events.
      }
    })();
    return () => unlisten?.();
  });

  // Watched-folder scans can land after the startup load (cold start: the
  // first scan of a 50k folder takes seconds; live: files dropped into a
  // watched folder while the app runs). The backend announces every
  // catalog-changing scan on `library-changed`; re-list throttled (leading
  // edge + trailing catch-up): the first event refreshes immediately, and a
  // burst coalesces to at most one more. A pure trailing debounce starves
  // here — the hash backfill emits per batch, faster than any window, so the
  // grid would not refresh until the WHOLE backfill ended (TASK-01-004 perf
  // run: 33 s to first thumbnail).
  const refreshLibraryThrottled = throttle(() => void refreshLibrary(), 250);
  $effect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen("library-changed", () => {
          refreshLibraryThrottled();
        });
      } catch {
        // Browser dev: no backend, no events.
      }
    })();
    return () => unlisten?.();
  });

  // Catalog switched (TASK-03-024): the backend swapped the active catalog
  // underneath the shell (from the tray menu). Everything the shell holds —
  // grid rows, search results, viewer state — belongs to the old catalog,
  // so a full reload re-reads the new one. No app restart required.
  $effect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen("catalog-switched", () => {
          location.reload();
        });
      } catch {
        // Browser dev: no backend, no events.
      }
    })();
    return () => unlisten?.();
  });

  // Folder ini offers (TASK-03-022): a scan that meets a folder's ini
  // announces it on `folder-ini-offered`; offers recorded earlier
  // (a previous session never answered) are read once at startup. Either
  // way the user decides — nothing migrates by itself.
  $effect(() => {
    let unlisten: (() => void) | null = null;
    const offer = (found: string[]): void => {
      const fresh = found.filter((f) => !folderIniFolders.includes(f));
      if (fresh.length === 0) return;
      folderIniFolders = [...folderIniFolders, ...fresh];
      folderIniOpen = true;
    };
    void (async () => {
      try {
        offer(await folderIniPendingFolders());
        unlisten = await onFolderIniOffered(offer);
      } catch {
        // Browser dev: no backend, no offers.
      }
    })();
    return () => unlisten?.();
  });

  // Library filtering (TASK-01-015 folders, TASK-01-018 live search): the
  // selected folder ids and the shared store's searchQuery combine — the
  // backend narrows in SQL (folder `IN` + FTS MATCH), so a 50k library
  // never ships every row over IPC just to hide most of it. Clearing the
  // search naturally restores the folder-filtered view: the query is the
  // only thing that changed.
  $effect(() => {
    // Read every filter up front so the effect tracks each of them. An
    // album view (TASK-03-008) takes the place of the folder filter; the
    // search box still narrows it.
    const sel = selectedFolders;
    const album = selectedAlbum;
    // The filter state is read here too (inside `loadPhotos`' synchronous
    // prefix), so toggling a button or moving the slider re-queries.
    const seq = ++loadSeq;
    void (async () => {
      try {
        const rows = await loadPhotos(sel, album);
        // A slower, older query must not overwrite a newer one's result.
        if (seq === loadSeq) photos = rows;
      } catch {
        // Browser dev / backend down: nothing to show.
        photos = [];
      }
    })();
  });

  // Context menus (TASK-01-021, PLAN §2 File menu / Folder context menu):
  // the photo menu carries the File-menu P1 entries (locate, delete to
  // trash, exit); the folder menu carries refresh / locate / remove-from-
  // catalog. Confirmations happen before anything touches the disk.
  const photoMenuItems = $derived<MenuItem[]>([
    { id: "view-slideshow", label: t("menu.viewSlideshow") },
    { id: "locate-on-disk", label: t("menu.locateOnDisk") },
    { id: "delete-from-disk", label: t("menu.deleteFromDisk") },
    { id: "add-to-album", label: t("menu.addToAlbum"), disabled: albums.length === 0 },
    { id: "adjust-date-time", label: t("menu.adjustDateTime") },
    {
      id: "stack-photos",
      label: t("menu.stackPhotos"),
      disabled:
        photoMenu.item === null ||
        !canStack(photosById(targetIds(photoMenu.item.id, gridState.selection))),
    },
    ...(photoMenu.item?.stack_id
      ? [
          { id: "show-stack", label: t("menu.showStack") },
          { id: "unstack", label: t("menu.unstack") },
        ]
      : []),
    photoMenu.item !== null && tray.has(photoMenu.item.id)
      ? { id: "remove-from-tray", label: t("menu.removeFromTray") }
      : { id: "add-to-tray", label: t("menu.addToTray") },
    photoMenu.item?.hidden
      ? { id: "unhide-photo", label: t("menu.unhide") }
      : { id: "hide-photo", label: t("menu.hide") },
    ...(selectedAlbum !== null
      ? [
          { id: "remove-from-album", label: t("albums.removeFromPicked") },
          { id: "set-album-cover", label: t("albums.setCover") },
        ]
      : []),
    { id: "exit", label: t("menu.exit") },
  ]);

  const albumMenuItems = $derived<MenuItem[]>([
    { id: "edit-album", label: t("albums.edit") },
    { id: "delete-album", label: t("albums.delete") },
  ]);

  // The photo menu's "Add to album" opens this picker: one entry per album,
  // then "New album…" (which adds the photos to the album once created).
  const albumPickerItems = $derived<MenuItem[]>([
    ...albums.map((a) => ({ id: `album:${a.id}`, label: a.name })),
    { id: "new-album", label: t("albums.newEllipsis") },
  ]);
  let pendingAlbumIds = $state<number[]>([]);

  const folderMenuItems = $derived<MenuItem[]>([
    { id: "refresh-thumbnails", label: t("menu.refreshThumbnails") },
    folderMenu.folder?.hidden
      ? { id: "unhide-folder", label: t("menu.unhideFolder") }
      : { id: "hide-folder", label: t("menu.hideFolder") },
    { id: "locate-on-disk", label: t("menu.locateOnDisk") },
    { id: "remove-from-fotobase", label: t("menu.removeFromFotobase") },
    { id: "exit", label: t("menu.exit") },
  ]);

  async function handlePhotoPick(id: string): Promise<void> {
    const item = photoMenu.item;
    if (!item) return;
    if (id === "view-slideshow") {
      openSlideshow();
    } else if (id === "locate-on-disk" || id === "delete-from-disk") {
      // Both need the on-disk path; the grid items carry only id + hash.
      try {
        const info = await loadPhotoInfo(item.id);
        if (!info) return;
        if (id === "locate-on-disk") {
          await revealInDisk(info.path);
        } else if (confirm(t("confirm.deletePhoto"))) {
          await deleteFromDisk(item.id);
          tray.remove(item.id);
          await refreshLibrary();
        }
      } catch {
        // Browser dev / backend down: nothing to reveal or delete.
      }
    } else if (id === "add-to-album") {
      albumPicker = {
        open: true,
        x: photoMenu.x,
        y: photoMenu.y,
        ids: targetIds(item.id, gridState.selection),
      };
    } else if (id === "adjust-date-time") {
      dateTimeDialog = { open: true, ids: targetIds(item.id, gridState.selection) };
    } else if (id === "stack-photos") {
      // Bursts and anything else the detector did not offer: the selection
      // (or the clicked photo) becomes one stack; the first becomes the top.
      const ids = targetIds(item.id, gridState.selection);
      const top = ids.includes(item.id) ? [item.id, ...ids.filter((i) => i !== item.id)] : ids;
      try {
        await stackPhotos(top);
      } catch {
        // Backend down or a sidecar refused the write: nothing was stacked.
      }
      gridState.selection = new Set();
      await refreshView();
    } else if (id === "show-stack" && item.stack_id) {
      stackPicker = { open: true, stackId: item.stack_id, x: photoMenu.x, y: photoMenu.y };
    } else if (id === "unstack" && item.stack_id) {
      try {
        await unstack(item.stack_id);
      } catch {
        // Backend down: the stack stays.
      }
      await refreshView();
    } else if (id === "add-to-tray") {
      // Same target rule as the other photo actions: the whole selection when
      // the clicked photo is part of it, otherwise just that photo.
      tray.add(photosById(targetIds(item.id, gridState.selection)));
    } else if (id === "remove-from-tray") {
      tray.remove(item.id);
    } else if (id === "remove-from-album" && selectedAlbum !== null) {
      try {
        await removeFromAlbum(selectedAlbum, targetIds(item.id, gridState.selection));
      } catch {
        // Backend down: the grid keeps showing the album as it was.
      }
      await refreshAlbums();
    } else if (id === "set-album-cover" && selectedAlbum !== null) {
      try {
        await setAlbumCover(selectedAlbum, item.id);
      } catch {
        // Backend down: the cover stays as it was.
      }
      await refreshAlbums();
    } else if (id === "hide-photo" || id === "unhide-photo") {
      // Library-only: the photo's own sidecar flag, nothing on disk moves.
      // Hiding is soft-deleted into the trash, so it can be undone from there.
      const ids = targetIds(item.id, gridState.selection);
      try {
        await (id === "hide-photo" ? hidePhotos(ids) : unhidePhotos(ids));
      } catch {
        // Backend down: nothing changed.
      }
      for (const hiddenId of ids) tray.remove(hiddenId);
      gridState.selection = new Set();
      await refreshView();
    } else if (id === "exit") {
      void exitApp().catch(() => {
        // The backend tears down mid-response on a clean exit — not an error.
      });
    }
  }

  function photosById(ids: number[]): TrayPhoto[] {
    const wanted = new Set(ids);
    return photos.filter((p) => wanted.has(p.id));
  }

  // Tray click (TASK-03-010): show the photo in the grid. When the current
  // folder/album/search view hides it, widen the view to the whole library
  // first (search and filters included); the grid scrolls to it as soon as
  // the reloaded list contains it.
  function jumpToPhoto(photo: TrayPhoto): void {
    if (!photos.some((p) => p.id === photo.id)) {
      selectedFolders = [];
      selectedAlbum = null;
      searchEl?.clear();
      gridState.clearFilters();
    }
    gridState.revealRequest = { id: photo.id };
  }

  // Tray "Add to album": the picker opens above the button — the tray sits at
  // the window bottom, a downward menu would fall off screen.
  function openTrayAlbumPicker(e: { ids: number[]; x: number; y: number }): void {
    const menuHeight = (albumPickerItems.length + 1) * 32;
    albumPicker = { open: true, x: e.x, y: Math.max(0, e.y - menuHeight), ids: e.ids };
  }

  // Files moved/renamed on disk: re-read the library so grid, tree and tray
  // show the new names and folders.
  async function handleBatchApplied(): Promise<void> {
    await refreshLibrary();
    tray.refresh(photos);
  }

  async function handleAlbumPick(id: string): Promise<void> {
    const ids = albumPicker.ids;
    if (id === "new-album") {
      pendingAlbumIds = ids;
      albumDialog = { open: true, album: null };
      return;
    }
    const albumId = Number(id.slice("album:".length));
    try {
      await addToAlbum(albumId, ids);
    } catch {
      // Backend down: nothing was added.
    }
    await refreshAlbums();
  }

  async function handleAlbumMenuPick(id: string): Promise<void> {
    const album = albumMenu.album;
    if (!album) return;
    if (id === "edit-album") {
      pendingAlbumIds = [];
      albumDialog = { open: true, album };
    } else if (id === "delete-album" && confirm(t("albums.deleteConfirm", { name: album.name }))) {
      // The album only — its photos stay in the library.
      try {
        await deleteAlbum(album.id);
      } catch {
        // Backend down: the album stays.
      }
      if (selectedAlbum === album.id) selectedAlbum = null;
      await refreshAlbums();
    }
  }

  // A new album created from the photo menu receives the photos it was
  // created for.
  async function handleAlbumSaved(album: Album): Promise<void> {
    const ids = pendingAlbumIds;
    pendingAlbumIds = [];
    if (ids.length > 0) {
      try {
        await addToAlbum(album.id, ids);
      } catch {
        // The album exists; adding the photos failed — they can be added again.
      }
    }
    await refreshAlbums();
  }

  async function handleFolderPick(id: string): Promise<void> {
    const folder = folderMenu.folder;
    if (!folder) return;
    try {
      if (id === "refresh-thumbnails") {
        await refreshFolderThumbnails(folder.path);
      } else if (id === "locate-on-disk") {
        await revealInDisk(folder.path);
      } else if (id === "hide-folder" || id === "unhide-folder") {
        await (id === "hide-folder" ? hideFolder(folder.path) : unhideFolder(folder.path));
        await refreshView();
      } else if (id === "remove-from-fotobase" && confirm(t("folderManager.remove.confirm"))) {
        // Catalog only — the files on disk are never touched.
        await removeFolder(folder.path);
        await refreshLibrary();
      } else if (id === "exit") {
        void exitApp().catch(() => {});
      }
    } catch {
      // Browser dev / backend down: nothing to do.
    }
  }

  function openPhotoMenu(e: { x: number; y: number; item: GridItem }): void {
    photoMenu = { open: true, x: e.x, y: e.y, item: e.item };
  }

  function openStackPicker(e: { stackId: string; x: number; y: number }): void {
    stackPicker = { open: true, ...e };
  }

  function openAlbumMenu(e: { album: Album; x: number; y: number }): void {
    albumMenu = { open: true, x: e.x, y: e.y, album: e.album };
  }

  function openFolderMenu(e: { folder: FolderInfo; x: number; y: number }): void {
    folderMenu = { open: true, x: e.x, y: e.y, folder: e.folder };
  }
</script>

<main use:shortcuts={globalShortcuts}>
  <h1>{t("app.title")}</h1>
  <p>{t("app.tagline")}</p>
  <button
    type="button"
    onclick={ping}
    title={settings?.show_tooltips ? t("app.ping.tooltip") : undefined}
  >
    {t("app.ping")}
  </button>
  {#if pong}
    <p>{t("app.backendSays")} <code>{pong}</code></p>
  {/if}
  <button type="button" onclick={() => (optionsOpen = true)} data-testid="open-options">
    {t("app.options")}
  </button>
  <button
    type="button"
    onclick={() => (folderManagerOpen = true)}
    data-testid="open-folder-manager"
  >
    {t("app.folderManager")}
  </button>
  <button type="button" onclick={() => (importOpen = true)} data-testid="open-import">
    {t("app.import")}
  </button>
  <button type="button" onclick={() => (trashOpen = true)} data-testid="open-trash">
    {t("app.trash")}
  </button>
  <button
    type="button"
    onclick={() => (stackSuggestionsOpen = true)}
    data-testid="open-stack-suggestions"
  >
    {t("app.stacks")}
  </button>
  {#if settings}
    <OptionsDialog
      bind:open={optionsOpen}
      settings={settings}
      onchange={(s) => {
        settings = s;
        setLocale(s.language);
      }}
      onrevealchanged={() => void refreshView()}
    />
  {/if}
  {#if settings}
    <FolderManagerDialog
      bind:open={folderManagerOpen}
      folders={folders}
      onchanged={refreshLibrary}
    />
    <ImportDialog bind:open={importOpen} onchanged={refreshLibrary} />
  {/if}
  <TrashDialog bind:open={trashOpen} onchanged={refreshView} />
  <FolderIniDialog
    bind:open={folderIniOpen}
    bind:folders={folderIniFolders}
    onmigrated={() => void refreshLibrary()}
  />
  <StackSuggestionsDialog bind:open={stackSuggestionsOpen} onapplied={() => void refreshView()} />
  <StackPicker
    bind:open={stackPicker.open}
    stackId={stackPicker.stackId}
    x={stackPicker.x}
    y={stackPicker.y}
    onchanged={() => void refreshView()}
  />
  <!-- Folders view + grid (TASK-01-015): the tree's multi-select is the
       grid's filter; both live in one flex row. -->
  <div class="library">
    <div class="sidebar">
      <FolderTree
        folders={folders}
        selected={selectedFolders}
        onselect={(ids) => {
          selectedFolders = ids;
          selectedAlbum = null;
        }}
        onfoldercontextmenu={openFolderMenu}
        options={folderView}
        onoptionschange={changeFolderView}
      />
      <AlbumList
        {albums}
        selected={selectedAlbum}
        onselect={(id) => (selectedAlbum = id)}
        oncreate={() => {
          pendingAlbumIds = [];
          albumDialog = { open: true, album: null };
        }}
        oncontextmenu={openAlbumMenu}
      />
    </div>
    <div class="grid-area">
      <!-- Live search (TASK-01-018): debounced into the shared store; the
           effect above re-queries FTS and both renderers narrow. -->
      <SearchBar bind:this={searchEl} />
      <!-- Filter buttons + date slider (TASK-03-014): same store-driven path
           as the search; they AND-combine with it and apply in the viewer. -->
      <FilterBar />
      <!-- Quick Tags (TASK-03-006, PLAN §2B): one click applies/removes one
           of the 10 configured tags on the whole selection. -->
      {#if settings && (gridState.selection.size > 0 || tray.count > 0)}
        <QuickTags
          tags={settings.quick_tags}
          selectedIds={actionIds(gridState.selection, tray.ids)}
          onapplied={() => void refreshLibrary()}
        />
      {/if}
      <!-- Renderer switch (TASK-01-014): config-driven, user-overridable. DOM
           stays the default everywhere; the canvas renderer is available on all
           OSes. Both read the same shared store, so zoom/selection/scroll carry
           across a switch. -->
      {#if settings && pickRenderer(settings.grid_renderer) === "canvas"}
        <CanvasThumbnailGrid
          items={photos}
          onvisible={prioritizeVisible}
          onopen={(item) => openViewer(item)}
          singleClickEdit={settings?.single_click_edit ?? false}
          oncontextmenu={openPhotoMenu}
          onopenstack={openStackPicker}
        />
      {:else}
        <ThumbnailGrid
          items={photos}
          onvisible={prioritizeVisible}
          onopen={(item) => openViewer(item)}
          singleClickEdit={settings?.single_click_edit ?? false}
          oncontextmenu={openPhotoMenu}
          onopenstack={openStackPicker}
        />
      {/if}
    </div>
  </div>
  <!-- Photo Tray (TASK-03-010): cross-folder selection at the window bottom.
       Quick Tags and "Add to album" apply to it while it has contents. -->
  <PhotoTray
    selectedIds={[...gridState.selection]}
    resolve={photosById}
    onjump={jumpToPhoto}
    onaddtoalbum={openTrayAlbumPicker}
    onbatch={(e) => (batchDialog = { open: true, ...e })}
  />
  {#if viewer.open && !edit.open}
    <Viewer
      items={photos}
      startIndex={viewer.index}
      startSlideshow={viewer.slideshow}
      ratingsEnabled={settings?.ratings_enabled ?? false}
      onrate={(id, rating) => {
        photos = photos.map((p) => (p.id === id ? { ...p, rating } : p));
      }}
      onalbumschanged={() => void refreshAlbums()}
      onclose={() => (viewer = { open: false, index: 0, slideshow: false })}
      onedit={openEdit}
    />
  {/if}
  {#if edit.open}
    <EditView
      items={photos}
      startIndex={edit.index}
      onclose={() => (edit = { open: false, index: 0 })}
    />
  {/if}
  <ContextMenu
    bind:open={folderMenu.open}
    x={folderMenu.x}
    y={folderMenu.y}
    items={folderMenuItems}
    onpick={handleFolderPick}
  />
  <ContextMenu
    bind:open={albumMenu.open}
    x={albumMenu.x}
    y={albumMenu.y}
    items={albumMenuItems}
    onpick={handleAlbumMenuPick}
  />
  <ContextMenu
    bind:open={albumPicker.open}
    x={albumPicker.x}
    y={albumPicker.y}
    items={albumPickerItems}
    onpick={handleAlbumPick}
  />
  <AlbumDialog
    bind:open={albumDialog.open}
    album={albumDialog.album}
    onsaved={handleAlbumSaved}
  />
  <DateTimeDialog
    bind:open={dateTimeDialog.open}
    ids={dateTimeDialog.ids}
    onapplied={() => void refreshLibrary()}
  />
  <BatchDialog
    bind:open={batchDialog.open}
    mode={batchDialog.mode}
    ids={batchDialog.ids}
    {folders}
    onapplied={handleBatchApplied}
  />
  <ContextMenu
    bind:open={photoMenu.open}
    x={photoMenu.x}
    y={photoMenu.y}
    items={photoMenuItems}
    onpick={handlePhotoPick}
  />
</main>
