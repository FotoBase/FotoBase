<script lang="ts">
  // Skeleton shell (library grid arrives with 01-013+). This carries the
  // first wiring of the Options dialog (TASK-01-022): settings load at
  // startup, the dialog persists every change, and the update-mode policy
  // runs once on startup.
  import OptionsDialog from "./lib/OptionsDialog.svelte";
  import FolderManagerDialog from "./lib/FolderManagerDialog.svelte";
  import ImportDialog from "./lib/ImportDialog.svelte";
  import ThumbnailGrid from "./lib/ThumbnailGrid.svelte";
  import CanvasThumbnailGrid from "./lib/CanvasThumbnailGrid.svelte";
  import FolderTree from "./lib/FolderTree.svelte";
  import SearchBar from "./lib/SearchBar.svelte";
  import ContextMenu from "./lib/ContextMenu.svelte";
  import Viewer from "./lib/Viewer.svelte";
  import { setLocale, t } from "./lib/i18n.svelte.js";
  import { loadSettings, type Settings } from "./lib/settings";
  import { runUpdateFlow } from "./lib/updates";
  import { applyTheme, resolveTheme, systemPrefersDark, watchSystemTheme } from "./lib/theme";
  import { shortcuts, type ShortcutHandlers } from "./lib/shortcuts";
  import { pickRenderer, gridState } from "./lib/grid-state.svelte.js";
  import { debounce, type GridItem } from "./lib/grid";
  import { listLibraryItems } from "./lib/search";
  import { deleteFromDisk, exitApp, refreshFolderThumbnails, revealInDisk } from "./lib/file-menu";
  import { loadPhotoInfo } from "./lib/info-pane";
  import { removeFolder } from "./lib/folder-manager";
  import type { FolderInfo, MenuItem } from "./lib/folder-tree";

  let pong = $state<string | null>(null);
  let optionsOpen = $state(false);
  let folderManagerOpen = $state(false);
  let importOpen = $state(false);
  let settings = $state<Settings | null>(null);
  let photos = $state<GridItem[]>([]);
  let folders = $state<FolderInfo[]>([]);
  let selectedFolders = $state<number[]>([]);
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
  let searchEl: { focus(): void } | null = $state(null);
  const globalShortcuts: ShortcutHandlers = {
    "search-focus": () => searchEl?.focus(),
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

  // Library refresh (TASK-01-016): the startup load and every Folder
  // Manager change (mode applied / folder removed) re-read folders + files,
  // so the grid mirrors the catalog immediately.
  async function refreshLibrary(): Promise<void> {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      photos = await invoke<GridItem[]>("list_files");
      folders = await invoke<FolderInfo[]>("list_folders");
    } catch {
      // Browser dev: no backend, empty grid and tree.
      photos = [];
      folders = [];
    }
  }

  $effect(() => {
    // Startup: load persisted settings, apply the language, then honor the
    // update policy (auto installs, check-but-ask prompts, off does
    // nothing — the backend gate enforces that too).
    void (async () => {
      const s = await loadSettings();
      settings = s;
      setLocale(s.language);
      await runUpdateFlow(s.update_mode);
      await refreshLibrary();
    })();
  });

  // Library filtering (TASK-01-015 folders, TASK-01-018 live search): the
  // selected folder ids and the shared store's searchQuery combine — the
  // backend narrows in SQL (folder `IN` + FTS MATCH), so a 50k library
  // never ships every row over IPC just to hide most of it. Clearing the
  // search naturally restores the folder-filtered view: the query is the
  // only thing that changed.
  $effect(() => {
    // Read both filters up front so the effect tracks each of them.
    const sel = selectedFolders;
    const query = gridState.searchQuery;
    void (async () => {
      try {
        photos = await listLibraryItems(sel, query);
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
    { id: "add-to-album", label: t("menu.addToAlbum"), disabled: true },
    { id: "exit", label: t("menu.exit") },
  ]);

  const folderMenuItems = $derived<MenuItem[]>([
    { id: "refresh-thumbnails", label: t("menu.refreshThumbnails") },
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
          await refreshLibrary();
        }
      } catch {
        // Browser dev / backend down: nothing to reveal or delete.
      }
    } else if (id === "exit") {
      void exitApp().catch(() => {
        // The backend tears down mid-response on a clean exit — not an error.
      });
    }
  }

  async function handleFolderPick(id: string): Promise<void> {
    const folder = folderMenu.folder;
    if (!folder) return;
    try {
      if (id === "refresh-thumbnails") {
        await refreshFolderThumbnails(folder.path);
      } else if (id === "locate-on-disk") {
        await revealInDisk(folder.path);
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
  {#if settings}
    <OptionsDialog
      bind:open={optionsOpen}
      settings={settings}
      onchange={(s) => {
        settings = s;
        setLocale(s.language);
      }}
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
  <!-- Folders view + grid (TASK-01-015): the tree's multi-select is the
       grid's filter; both live in one flex row. -->
  <div class="library">
    <FolderTree
      folders={folders}
      selected={selectedFolders}
      onselect={(ids) => (selectedFolders = ids)}
      onfoldercontextmenu={openFolderMenu}
    />
    <div class="grid-area">
      <!-- Live search (TASK-01-018): debounced into the shared store; the
           effect above re-queries FTS and both renderers narrow. -->
      <SearchBar bind:this={searchEl} />
      <!-- Renderer switch (TASK-01-014): config-driven, user-overridable. DOM
           stays the default everywhere; the canvas renderer is available on all
           OSes. Both read the same shared store, so zoom/selection/scroll carry
           across a switch. -->
      {#if settings && pickRenderer(settings.grid_renderer) === "canvas"}
        <CanvasThumbnailGrid
          items={photos}
          onvisible={prioritizeVisible}
          onopen={(item) => openViewer(item)}
          oncontextmenu={openPhotoMenu}
        />
      {:else}
        <ThumbnailGrid
          items={photos}
          onvisible={prioritizeVisible}
          onopen={(item) => openViewer(item)}
          oncontextmenu={openPhotoMenu}
        />
      {/if}
    </div>
  </div>
  {#if viewer.open}
    <Viewer
      items={photos}
      startIndex={viewer.index}
      startSlideshow={viewer.slideshow}
      onclose={() => (viewer = { open: false, index: 0, slideshow: false })}
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
    bind:open={photoMenu.open}
    x={photoMenu.x}
    y={photoMenu.y}
    items={photoMenuItems}
    onpick={handlePhotoPick}
  />
</main>
