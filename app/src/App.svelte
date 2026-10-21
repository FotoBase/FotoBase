<script lang="ts">
  // Skeleton shell (library grid arrives with 01-013+). This carries the
  // first wiring of the Options dialog (TASK-01-022): settings load at
  // startup, the dialog persists every change, and the update-mode policy
  // runs once on startup.
  import OptionsDialog from "./lib/OptionsDialog.svelte";
  import ThumbnailGrid from "./lib/ThumbnailGrid.svelte";
  import CanvasThumbnailGrid from "./lib/CanvasThumbnailGrid.svelte";
  import { setLocale, t } from "./lib/i18n.svelte.js";
  import { loadSettings, type Settings } from "./lib/settings";
  import { runUpdateFlow } from "./lib/updates";
  import { pickRenderer } from "./lib/grid-state.svelte.js";
  import { debounce, type GridItem } from "./lib/grid";

  let pong = $state<string | null>(null);
  let optionsOpen = $state(false);
  let settings = $state<Settings | null>(null);
  let photos = $state<GridItem[]>([]);

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

  $effect(() => {
    // Startup: load persisted settings, apply the language, then honor the
    // update policy (auto installs, check-but-ask prompts, off does
    // nothing — the backend gate enforces that too).
    void (async () => {
      const s = await loadSettings();
      settings = s;
      setLocale(s.language);
      await runUpdateFlow(s.update_mode);
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        photos = await invoke<GridItem[]>("list_files");
      } catch {
        // Browser dev: no backend, empty grid.
        photos = [];
      }
    })();
  });
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
  <!-- Renderer switch (TASK-01-014): config-driven, user-overridable. DOM
       stays the default everywhere; the canvas renderer is available on all
       OSes. Both read the same shared store, so zoom/selection/scroll carry
       across a switch. -->
  {#if settings && pickRenderer(settings.grid_renderer) === "canvas"}
    <CanvasThumbnailGrid items={photos} onvisible={prioritizeVisible} />
  {:else}
    <ThumbnailGrid items={photos} onvisible={prioritizeVisible} />
  {/if}
</main>
