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
  import ContextMenu from "./lib/ContextMenu.svelte";
  import { setLocale, t } from "./lib/i18n.svelte.js";
  import { loadSettings, type Settings } from "./lib/settings";
  import { runUpdateFlow } from "./lib/updates";
  import { pickRenderer } from "./lib/grid-state.svelte.js";
  import { debounce, type GridItem } from "./lib/grid";
  import type { FolderInfo, MenuItem } from "./lib/folder-tree";

  let pong = $state<string | null>(null);
  let optionsOpen = $state(false);
  let folderManagerOpen = $state(false);
  let importOpen = $state(false);
  let settings = $state<Settings | null>(null);
  let photos = $state<GridItem[]>([]);
  let folders = $state<FolderInfo[]>([]);
  let selectedFolders = $state<number[]>([]);
  let folderMenu = $state<{ open: boolean; x: number; y: number }>({
    open: false,
    x: 0,
    y: 0,
  });

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

  // Folder filtering (TASK-01-015): the selected folder ids are the grid's
  // filter; the backend narrows by folder_id in SQL. Re-runs whenever the
  // tree selection changes.
  $effect(() => {
    const sel = selectedFolders;
    void (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        photos = sel.length
          ? await invoke<GridItem[]>("list_files", { folders: sel })
          : await invoke<GridItem[]>("list_files");
      } catch {
        // Browser dev / backend down: nothing to show.
        photos = [];
      }
    })();
  });

  // Context-menu skeleton (TASK-01-015): P1 entries emit and are handled
  // where a trivial handler exists; the rest stay wired no-ops until their
  // commands land (slideshow P1 viewer, locate/remove = 01-021).
  const menuItems = $derived<MenuItem[]>([
    { id: "view-slideshow", label: t("menu.viewSlideshow") },
    { id: "refresh-thumbnails", label: t("menu.refreshThumbnails") },
    { id: "locate-on-disk", label: t("menu.locateOnDisk") },
    { id: "remove-from-fotobase", label: t("menu.removeFromFotobase") },
    { id: "add-to-album", label: t("menu.addToAlbum"), disabled: true },
  ]);

  function handleMenuPick(id: string): void {
    if (id === "refresh-thumbnails") {
      void (async () => {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          await invoke("clear_cache");
        } catch {
          // Browser dev / backend down: nothing to refresh.
        }
      })();
    }
    // view-slideshow / locate-on-disk / remove-from-fotobase: wired no-ops
    // until the viewer and the 01-021 file-menu commands exist.
  }

  function openFolderMenu(e: { folder: FolderInfo; x: number; y: number }): void {
    folderMenu = { open: true, x: e.x, y: e.y };
  }
</script>

<main>
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
      <!-- Renderer switch (TASK-01-014): config-driven, user-overridable. DOM
           stays the default everywhere; the canvas renderer is available on all
           OSes. Both read the same shared store, so zoom/selection/scroll carry
           across a switch. -->
      {#if settings && pickRenderer(settings.grid_renderer) === "canvas"}
        <CanvasThumbnailGrid items={photos} onvisible={prioritizeVisible} />
      {:else}
        <ThumbnailGrid items={photos} onvisible={prioritizeVisible} />
      {/if}
    </div>
  </div>
  <ContextMenu
    bind:open={folderMenu.open}
    x={folderMenu.x}
    y={folderMenu.y}
    items={menuItems}
    onpick={handleMenuPick}
  />
</main>
