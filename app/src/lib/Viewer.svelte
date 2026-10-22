<!--
  Single-photo viewer (TASK-01-019, PLAN §2D — P1 scope is view-only).
  Opens over the grid with the current folder/search selection as its
  collection: the main image is a `preview://` URI (full-size bytes fetched
  straight from disk by the WebView, PLAN §3 rule 3 — no image bytes over
  IPC), the filmstrip shows `thumb://` thumbnails for the whole collection.

  Keyboard: ←/→ navigate (and pause a running slideshow), Esc exits,
  F toggles fullscreen (Tauri window API), space toggles the slideshow.
-->
<script lang="ts">
  import { thumbUri, type GridItem } from "./grid";
  import { nextIndex, preloadUris, previewUri, prevIndex } from "./viewer";
  import { t } from "./i18n.svelte.js";

  interface Props {
    items?: GridItem[];
    /// Which photo the grid opened (index into `items`).
    startIndex?: number;
    /// Slideshow interval in ms (functional spec: configurable interval).
    slideshowMs?: number;
    /// The context menu's "View slideshow" opens with the slideshow running.
    startSlideshow?: boolean;
    onclose?: () => void;
  }

  let {
    items = [],
    startIndex = 0,
    slideshowMs = 3000,
    startSlideshow = false,
    onclose = () => {},
  }: Props = $props();

  let index = $state(Math.max(0, Math.min(startIndex, items.length - 1)));
  let slideshowRunning = $state(startSlideshow);
  let fullscreen = $state(false);

  const current = $derived(items[index] ?? null);
  const mainUri = $derived(previewUri(current?.hash ?? null));

  // Preload the neighbors (functional spec: navigation feels instant). The
  // browser fetches them through the preview:// scheme handler; nothing
  // crosses IPC.
  $effect(() => {
    void index;
    void items;
    const uris = preloadUris(
      items.map((it) => previewUri(it.hash)),
      index,
    );
    for (const uri of uris) {
      const img = new Image();
      img.src = uri;
    }
  });

  // Slideshow timer (technical spec: frontend-only). Re-created whenever the
  // running flag or the interval changes; torn down on pause/exit.
  $effect(() => {
    if (!slideshowRunning) return;
    const timer = setInterval(() => {
      index = nextIndex(index, items.length);
    }, slideshowMs);
    return () => clearInterval(timer);
  });

  async function toggleFullscreen(): Promise<void> {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      fullscreen = !fullscreen;
      await getCurrentWindow().setFullscreen(fullscreen);
    } catch {
      // Browser dev: no Tauri window API — stay windowed, never crash.
    }
  }

  function onKeydown(e: KeyboardEvent): void {
    switch (e.key) {
      case "ArrowRight":
        slideshowRunning = false; // pauses on interaction
        index = nextIndex(index, items.length);
        break;
      case "ArrowLeft":
        slideshowRunning = false;
        index = prevIndex(index, items.length);
        break;
      case "Escape":
        onclose();
        break;
      case "f":
      case "F":
        void toggleFullscreen();
        break;
      case " ":
        e.preventDefault();
        slideshowRunning = !slideshowRunning;
        break;
    }
  }

  $effect(() => {
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });
</script>

<div class="viewer" data-testid="viewer">
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="viewer-stage" data-testid="viewer-stage">
    {#if mainUri}
      <img class="viewer-main" data-testid="viewer-main" src={mainUri} alt={current?.filename ?? ""} draggable="false" />
    {:else}
      <div class="viewer-placeholder" data-testid="viewer-placeholder"></div>
    {/if}
  </div>
  <div class="viewer-status">
    <span data-testid="viewer-count">{index + 1}/{items.length}</span>
    {#if slideshowRunning}
      <span data-testid="slideshow-running">{t("viewer.slideshowRunning")}</span>
    {/if}
  </div>
  <div class="filmstrip" data-testid="filmstrip">
    {#each items as item, i (item.id)}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <img
        class="filmstrip-thumb{i === index ? ' current' : ''}"
        data-testid="filmstrip-thumb"
        src={thumbUri(item.hash) ?? ""}
        alt={item.filename}
        draggable="false"
        onclick={() => {
          slideshowRunning = false;
          index = i;
        }}
      />
    {/each}
  </div>
</div>
