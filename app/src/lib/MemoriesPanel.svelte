<!--
  "On this day" memories (TASK-07-008, PLAN §2 2026 additions): photos taken
  on today's calendar day in other years, one section per year, newest year
  first. Read-only browsing — opening a photo hands it to the shell's
  normal viewer. Thumbnails are `thumb://` URIs from the disk cache (no image
  bytes over IPC); the sets are small (one day per year), so plain lazy
  images suffice and the grid's global scroll/zoom state stays untouched.
-->
<script lang="ts">
  import type { GridItem } from "./grid";
  import { thumbUri } from "./grid";
  import { t } from "./i18n.svelte.js";
  import {
    flattenMemories,
    loadMemories,
    localToday,
    type MemoryYear,
  } from "./memories";
  import type { LibraryView } from "./search";

  let {
    open = $bindable(false),
    view = undefined,
    onopenphoto = () => {},
  }: {
    open?: boolean;
    /// The grid's current view, so existing filters apply.
    view?: LibraryView;
    /// A photo was activated: the shell opens the viewer over `all`.
    onopenphoto?: (item: GridItem, all: GridItem[]) => void;
  } = $props();

  let years = $state<MemoryYear[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    if (!open) return;
    const current = view;
    loaded = false;
    error = null;
    years = [];
    void (async () => {
      try {
        years = await loadMemories(localToday(), current);
      } catch (e) {
        error = t("memories.error", { error: String(e) });
      } finally {
        loaded = true;
      }
    })();
  });

  function openPhoto(item: GridItem): void {
    onopenphoto(item, flattenMemories(years));
  }
</script>

{#if open}
  <div class="mem-backdrop" data-testid="memories-backdrop">
    <div class="mem-dialog" role="dialog" aria-label={t("memories.title")}>
      <h2>{t("memories.title")}</h2>
      {#if loaded && years.length === 0 && !error}
        <p data-testid="memories-empty">{t("memories.empty")}</p>
      {/if}
      {#each years as group (group.year)}
        <section data-testid="memories-year" data-year={group.year}>
          <h3>{t("memories.yearHeading", { year: group.year })}</h3>
          <div class="mem-grid">
            {#each group.files as item (item.id)}
              <button
                type="button"
                class="mem-cell"
                data-testid="memories-photo"
                aria-label={item.filename}
                onclick={() => openPhoto(item)}
              >
                {#if thumbUri(item.hash)}
                  <img src={thumbUri(item.hash) ?? ""} alt={item.filename} loading="lazy" draggable="false" />
                {:else}
                  <span class="mem-placeholder"></span>
                {/if}
              </button>
            {/each}
          </div>
        </section>
      {/each}
      {#if error}
        <p class="mem-error" role="alert">{error}</p>
      {/if}
      <div class="mem-footer">
        <button type="button" data-testid="memories-close" onclick={() => (open = false)}>
          {t("memories.close")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .mem-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: var(--fb-backdrop, rgb(0 0 0 / 40%));
    display: grid;
    place-items: center;
  }
  .mem-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    min-width: 32rem;
    max-width: 90vw;
    max-height: 90vh;
    overflow-y: auto;
    padding: 1rem 1.25rem;
    box-shadow: 0 8px 32px var(--fb-shadow, rgb(0 0 0 / 30%));
    text-align: left;
  }
  h2,
  h3,
  p {
    margin: 0 0 0.6rem;
  }
  section {
    margin-bottom: 1rem;
  }
  .mem-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, 120px);
    gap: 8px;
  }
  .mem-cell {
    width: 120px;
    height: 120px;
    padding: 0;
    border: 1px solid var(--fb-border, #ccc);
    border-radius: 4px;
    overflow: hidden;
    background: var(--fb-input-bg, #eee);
    cursor: pointer;
  }
  .mem-cell img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .mem-placeholder {
    display: block;
    width: 100%;
    height: 100%;
  }
  .mem-error {
    color: var(--fb-danger, #b00020);
  }
  .mem-footer {
    display: flex;
    justify-content: flex-end;
  }
</style>
