<!--
  Full-screen timeline (TASK-08-018, PLAN §2 §B): chronological browsing on
  ONE canvas — a DOM timeline of 50k items would not hold 60 fps, so this
  reuses the canvas grid's technique: a sticky canvas over a spacer that
  reserves the full horizontal scroll range, drawing only the groups in view.
  Groups are days, oldest first, spaced by the time between them; the view
  respects the grid's filters. Read-only: thumbnails are `thumb://` URIs
  decoded by the WebView (no image bytes over IPC, §3 rule 3). Arrow keys
  scrub, Enter or a click opens the grid at that day.
-->
<script lang="ts">
  import { tick } from "svelte";
  import { thumbUri } from "./grid";
  import { gridState } from "./grid-state.svelte.js";
  import { t } from "./i18n.svelte.js";
  import type { LibraryView } from "./search";
  import {
    CELL,
    decodeThumb,
    groupIndexAt,
    layoutTimeline,
    loadTimeline,
    scrollLeftFor,
    stepIndex,
    visibleRange,
    type TimelineGroup,
  } from "./timeline";

  /// What `drawImage` accepts; tests inject stand-ins with the same shape.
  type ImageLike = { src: string; width: number; height: number };

  let {
    open = $bindable(false),
    view = undefined,
    loadImage = decodeThumb,
    onopengroup = () => {},
  }: {
    open?: boolean;
    /// The grid's current view, so its filters apply.
    view?: LibraryView;
    loadImage?: (uri: string) => Promise<ImageLike | null>;
    /// A day was activated: the shell shows the grid at that date.
    onopengroup?: (group: TimelineGroup) => void;
  } = $props();

  /// Decoded thumbnails kept at once: the cache must not grow with the
  /// library (idle-RAM budget), only with what scrubbing recently showed.
  const IMAGE_CACHE_MAX = 600;
  const AXIS_H = 30;
  const CELL_Y = AXIS_H + 16;

  let groups = $state<TimelineGroup[]>([]);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let focused = $state(-1);
  let scrollLeft = $state(0);
  let scrollEl = $state<HTMLDivElement | null>(null);
  let canvasEl = $state<HTMLCanvasElement | null>(null);
  /// Bumped when a decoded image lands, to repaint without other inputs.
  let imageTick = $state(0);

  const layout = $derived(layoutTimeline(groups));
  const focusedGroup = $derived(focused >= 0 ? (groups[focused] ?? null) : null);

  const imgCache = new Map<string, ImageLike | "loading" | "failed">();

  function remember(uri: string, entry: ImageLike | "failed"): void {
    imgCache.delete(uri);
    imgCache.set(uri, entry);
    while (imgCache.size > IMAGE_CACHE_MAX) {
      const oldest = imgCache.keys().next().value;
      if (oldest === undefined) break;
      imgCache.delete(oldest);
    }
  }

  $effect(() => {
    if (!open) return;
    const current = view;
    loaded = false;
    error = null;
    groups = [];
    focused = -1;
    imgCache.clear();
    void (async () => {
      try {
        groups = await loadTimeline(current);
        // Start at the newest day: that is where a library is browsed from.
        focused = groups.length - 1;
      } catch (e) {
        error = t("timeline.error", { error: String(e) });
      } finally {
        loaded = true;
      }
      await tick();
      scrollEl?.focus();
      if (scrollEl && focused >= 0) {
        scrollEl.scrollLeft = scrollLeftFor(layout, focused, scrollEl.clientWidth);
        scrollLeft = scrollEl.scrollLeft;
      }
    })();
  });

  $effect(() => {
    const canvas = canvasEl;
    const container = scrollEl;
    if (!canvas || !container) return;
    const lay = layout;
    const left = scrollLeft;
    const sel = focused;
    void imageTick;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const dpr = window.devicePixelRatio || 1;
    const w = container.clientWidth;
    const h = container.clientHeight;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const css = getComputedStyle(document.documentElement);
    const color = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
    const text = color("--fb-text", "#ddd");
    const muted = color("--fb-muted", "#9a9a9a");
    const accent = color("--fb-accent", "#4a9eff");
    const surface = color("--fb-surface", "#2a2a2a");

    // Axis: year / month ticks that intersect the viewport.
    ctx.textBaseline = "middle";
    for (const tk of lay.ticks) {
      if (tk.x < left - 80 || tk.x > left + w) continue;
      const x = tk.x - left;
      ctx.fillStyle = tk.kind === "year" ? text : muted;
      ctx.font = tk.kind === "year" ? "bold 14px sans-serif" : "12px sans-serif";
      ctx.fillText(tk.label, x, AXIS_H / 2);
      ctx.fillStyle = muted;
      ctx.fillRect(x, AXIS_H - 4, 1, tk.kind === "year" ? 12 : 6);
    }

    const [start, end] = visibleRange(lay, left, w);
    ctx.font = "11px sans-serif";
    for (let i = start; i < end; i++) {
      const g = lay.groups[i];
      const x = lay.xs[i] - left;
      const uri = thumbUri(g.hash, g.hash ? (gridState.thumbVersions[g.hash] ?? 0) : 0);
      const entry = uri ? imgCache.get(uri) : undefined;
      if (entry && entry !== "loading" && entry !== "failed") {
        ctx.drawImage(entry as unknown as CanvasImageSource, x, CELL_Y, CELL, CELL);
      } else {
        ctx.fillStyle = surface;
        ctx.fillRect(x, CELL_Y, CELL, CELL);
        if (uri && !entry) {
          imgCache.set(uri, "loading");
          loadImage(uri)
            .then((img) => remember(uri, img ?? "failed"))
            .catch(() => remember(uri, "failed"))
            .finally(() => imageTick++);
        }
      }
      if (g.count > 1) {
        const label = String(g.count);
        const bw = ctx.measureText(label).width + 10;
        ctx.fillStyle = "rgba(0, 0, 0, 0.65)";
        ctx.fillRect(x + CELL - bw - 4, CELL_Y + 4, bw, 16);
        ctx.fillStyle = "#fff";
        ctx.fillText(label, x + CELL - bw + 1, CELL_Y + 12);
      }
      ctx.fillStyle = i === sel ? text : muted;
      ctx.fillText(g.date, x, CELL_Y + CELL + 14);
      if (i === sel) {
        ctx.strokeStyle = accent;
        ctx.lineWidth = 3;
        ctx.strokeRect(x - 1, CELL_Y - 1, CELL + 2, CELL + 2);
      }
    }
  });

  function onScroll(): void {
    if (scrollEl) scrollLeft = scrollEl.scrollLeft;
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      open = false;
      return;
    }
    if ((e.key === "Enter" || e.key === " ") && focusedGroup) {
      e.preventDefault();
      onopengroup(focusedGroup);
      return;
    }
    const next = stepIndex(focused, e.key, groups.length);
    if (next === null) return;
    e.preventDefault();
    focused = next;
    if (scrollEl) {
      // Scroll only when the group leaves the viewport — arrow-scrubbing
      // within the screen keeps the picture still.
      const x = layout.xs[next];
      if (x < scrollEl.scrollLeft || x + CELL > scrollEl.scrollLeft + scrollEl.clientWidth) {
        scrollEl.scrollLeft = scrollLeftFor(layout, next, scrollEl.clientWidth);
      }
    }
  }

  function onClick(e: MouseEvent): void {
    if (!scrollEl) return;
    const i = groupIndexAt(layout, e.offsetX + scrollEl.scrollLeft);
    if (i < 0) return;
    focused = i;
    onopengroup(groups[i]);
  }

  function photoCount(g: TimelineGroup): string {
    return t(g.count === 1 ? "timeline.photoOne" : "timeline.photoMany", { count: g.count });
  }
</script>

{#if open}
  <div class="tl-overlay" role="dialog" aria-modal="true" aria-label={t("timeline.title")} data-testid="timeline">
    <header class="tl-header">
      <h2>{t("timeline.title")}</h2>
      <span class="tl-status" data-testid="timeline-status" aria-live="polite">
        {#if focusedGroup}{focusedGroup.date} · {photoCount(focusedGroup)}{/if}
      </span>
      <span class="tl-hint">{t("timeline.hint")}</span>
      <button type="button" data-testid="timeline-close" onclick={() => (open = false)}>
        {t("timeline.close")}
      </button>
    </header>
    {#if loaded && groups.length === 0 && !error}
      <p class="tl-message" data-testid="timeline-empty">{t("timeline.empty")}</p>
    {/if}
    {#if error}
      <p class="tl-message tl-error" role="alert">{error}</p>
    {/if}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="tl-scroll"
      data-testid="timeline-scroll"
      data-focused-date={focusedGroup?.date ?? ""}
      role="application"
      aria-label={t("timeline.title")}
      tabindex="0"
      bind:this={scrollEl}
      onscroll={onScroll}
      onkeydown={onKeydown}
    >
      <div class="tl-spacer" style="width: {layout.totalWidth}px" data-testid="timeline-spacer"></div>
      <canvas class="tl-canvas" data-testid="timeline-canvas" bind:this={canvasEl} onclick={onClick}></canvas>
    </div>
  </div>
{/if}

<style>
  .tl-overlay {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: flex;
    flex-direction: column;
    background: var(--fb-bg, #1b1b1b);
    color: var(--fb-text, #ddd);
  }
  .tl-header {
    display: flex;
    align-items: baseline;
    gap: 1rem;
    padding: 0.6rem 1rem;
    border-bottom: 1px solid var(--fb-border, #444);
  }
  .tl-header h2 {
    margin: 0;
    font-size: 1.1rem;
  }
  .tl-status {
    flex: 1;
    font-variant-numeric: tabular-nums;
  }
  .tl-hint {
    color: var(--fb-muted, #9a9a9a);
    font-size: 0.85rem;
  }
  .tl-message {
    margin: 1rem;
  }
  .tl-error {
    color: var(--fb-danger, #b00020);
  }
  .tl-scroll {
    position: relative;
    flex: 1;
    overflow-x: auto;
    overflow-y: hidden;
  }
  .tl-scroll:focus-visible {
    outline: 2px solid var(--fb-accent, #4a9eff);
    outline-offset: -2px;
  }
  /* The spacer reserves the full scroll range; the canvas stays pinned to
     the viewport (sticky) and repaints only what is on screen. */
  .tl-spacer {
    position: absolute;
    top: 0;
    left: 0;
    height: 1px;
  }
  .tl-canvas {
    position: sticky;
    left: 0;
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
