<!--
  Canvas thumbnail grid (TASK-01-014, PLAN §6 P1): draws thumbnails to ONE
  canvas instead of DOM nodes. The canvas is sticky over a spacer that
  reserves the full scroll range, so the backing store stays viewport-sized
  however deep the collection is. Only visible cells are drawn per frame —
  the same bounded-work contract as the DOM grid.

  Thumbnails reuse the `thumb://` disk cache through the custom scheme
  handler (PLAN §3 rule 3): the WebView fetches and decodes the bytes into
  the render layer — still no image bytes over IPC. Zoom, selection, and
  scroll position live in the shared grid store, so this renderer and the
  DOM one stay in sync feature-wise and switching between them loses nothing.
-->
<script lang="ts">
  import {
    Virtualizer,
    elementScroll,
    observeElementOffset,
    observeElementRect,
    type VirtualItem,
  } from "@tanstack/virtual-core";
  import { t } from "./i18n.svelte.js";
  import { untrack } from "svelte";
  import {
    cellAt,
    columnCount,
    debounce,
    rowItems,
    thumbUri,
    visibleHashes,
    GRID_GAP,
    type GridItem,
  } from "./grid";
  import { gridState, ZOOM_MAX, ZOOM_MIN, ZOOM_STEP } from "./grid-state.svelte.js";

  /// What `drawImage` accepts; tests inject stand-ins with the same shape.
  type ImageLike = { src: string; width: number; height: number };

  interface Props {
    items?: GridItem[];
    overscan?: number;
    /// Decodes a `thumb://` URI into a drawable image. Injectable so tests
    /// can stand in for the browser decoder (jsdom never loads images).
    loadImage?: (uri: string) => Promise<ImageLike | null>;
    /// Called (debounced) with the unique hashes on screen.
    onvisible?: (hashes: string[]) => void;
    /// Called with the event coordinates and the item under the pointer.
    oncontextmenu?: (e: { x: number; y: number; item: GridItem }) => void;
  }

  let {
    items = [],
    overscan = 2,
    loadImage = async () => null,
    onvisible = () => {},
    oncontextmenu = () => {},
  }: Props = $props();

  const REPORT_DEBOUNCE_MS = 100;

  let scrollEl = $state<HTMLDivElement | null>(null);
  let canvasEl = $state<HTMLCanvasElement | null>(null);
  let virtualRows = $state<VirtualItem[]>([]);
  let totalSize = $state(0);
  let columns = $state(1);

  const reportVisible = debounce((hashes: string[]) => onvisible(hashes), REPORT_DEBOUNCE_MS);

  // Resume where the last renderer (or session) left off. Mount-only and
  // untracked — the virtualizer writes the store on every scroll, and a
  // reactive read here would re-run the virtualizer on every scroll event.
  $effect(() => {
    const el = scrollEl;
    if (!el) return;
    el.scrollTop = untrack(() => gridState.scrollTop);
  });

  /// Decoded-image cache: one fetch + decode per thumbnail, reused on every
  /// frame. `loading`/`failed` pins avoid stampeding the cache on repaints.
  const imgCache = new Map<string, ImageLike | "loading" | "failed">();

  /// The latest draw closure, so image-load callbacks can trigger a repaint
  /// without going through effect reactivity (avoids self-re-running loops).
  let currentDraw: (() => void) | null = null;

  $effect(() => {
    const el = scrollEl;
    if (!el) return;
    // Reactive inputs: zoom level and collection size rebuild the
    // virtualizer; scroll position is restored from the shared store.
    const size = gridState.thumbSize;
    void items.length;

    const cols = columnCount(el.clientWidth, size, GRID_GAP);
    columns = cols;
    const rowCount = Math.ceil(items.length / cols);

    const virtualizer = new Virtualizer({
      getScrollElement: () => el,
      count: rowCount,
      estimateSize: () => size + GRID_GAP,
      overscan,
      observeElementOffset,
      observeElementRect,
      scrollToFn: elementScroll,
      onChange: (inst) => {
        // Read the rows from the instance, not from the `virtualRows`
        // $state: this callback runs synchronously inside the effect, and
        // reading that state here would make it an effect dependency that
        // the same effect writes — an infinite re-run loop.
        virtualRows = inst.getVirtualItems();
        totalSize = inst.getTotalSize();
        gridState.scrollTop = inst.scrollOffset ?? 0;
        reportVisible(visibleHashes(items, cols, inst.getVirtualItems().map((r) => r.index)));
      },
    });
    const unmount = virtualizer._didMount();
    virtualizer._willUpdate();
    virtualRows = virtualizer.getVirtualItems();
    totalSize = virtualizer.getTotalSize();

    reportVisible(visibleHashes(items, cols, virtualizer.getVirtualItems().map((r) => r.index)));

    return () => {
      unmount();
    };
  });

  // Repaint whenever the visible range, zoom, or selection changes. Reads
  // only reactive inputs, writes none — no self-re-run loop.
  $effect(() => {
    const canvas = canvasEl;
    if (!canvas) return;
    void virtualRows;
    const size = gridState.thumbSize;
    void gridState.selection.size;

    const draw = (): void => {
      const ctx = canvas.getContext("2d");
      const container = scrollEl;
      if (!ctx || !container) return;
      const dpr = window.devicePixelRatio || 1;
      const w = container.clientWidth;
      const h = container.clientHeight;
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);

      const step = size + GRID_GAP;
      // Clamp to the scrollable range: row starts come from the virtualizer
      // (which clamps to the collection), so an out-of-range scrollTop must
      // not push every cell off-canvas.
      const scrollTop = Math.max(0, Math.min(container.scrollTop, totalSize - h));
      const selection = gridState.selection;

      for (const row of virtualRows) {
        const rowItemsInRow = rowItems(items, columns, row.index);
        for (let col = 0; col < rowItemsInRow.length; col++) {
          const item = rowItemsInRow[col];
          const x = col * step;
          const y = row.start - scrollTop;
          if (y + size < 0 || y > h) continue; // overscan guard: offscreen cells

          const uri = thumbUri(item.hash);
          const entry = uri ? imgCache.get(uri) : undefined;
          if (entry && entry !== "loading" && entry !== "failed") {
            ctx.drawImage(entry as unknown as CanvasImageSource, x, y, size, size);
          } else {
            // Placeholder first; the real thumb replaces it on next repaint.
            ctx.fillStyle = "#2a2a2a";
            ctx.fillRect(x, y, size, size);
            if (uri && !entry) {
              imgCache.set(uri, "loading");
              loadImage(uri)
                .then((img) => {
                  if (img) imgCache.set(uri, img);
                  else imgCache.set(uri, "failed");
                  currentDraw?.();
                })
                .catch(() => {
                  imgCache.set(uri, "failed");
                  currentDraw?.();
                });
            }
          }
        }
      }

      // Selection highlight (shared with the DOM renderer's store).
      if (selection.size > 0) {
        ctx.strokeStyle = "#4a9eff";
        ctx.lineWidth = 2;
        for (const id of selection) {
          const index = items.findIndex((it) => it.id === id);
          if (index < 0) continue;
          const row = Math.floor(index / columns);
          const col = index % columns;
          const x = col * step;
          const y = row * step - scrollTop;
          if (y + size < 0 || y > h) continue;
          ctx.strokeRect(x + 1, y + 1, size - 2, size - 2);
        }
      }
    };

    currentDraw = draw;
    draw();
  });

  function onClick(e: MouseEvent): void {
    if (!scrollEl) return;
    const index = cellAt(
      e.offsetX,
      e.offsetY,
      scrollEl.scrollTop,
      gridState.thumbSize,
      columns,
      items.length,
    );
    if (index === null) return;
    gridState.toggleSelection(items[index].id);
  }

  function onContextMenu(e: MouseEvent): void {
    if (!scrollEl) return;
    e.preventDefault();
    const index = cellAt(
      e.offsetX,
      e.offsetY,
      scrollEl.scrollTop,
      gridState.thumbSize,
      columns,
      items.length,
    );
    if (index === null) return;
    oncontextmenu({ x: e.clientX, y: e.clientY, item: items[index] });
  }
</script>

<div class="grid-shell">
  <div class="grid-toolbar">
    <label for="zoom-slider">{t("grid.zoom")}</label>
    <input
      id="zoom-slider"
      data-testid="zoom-slider"
      type="range"
      min={ZOOM_MIN}
      max={ZOOM_MAX}
      step={ZOOM_STEP}
      bind:value={gridState.thumbSize}
    />
  </div>
  <div class="grid-scroll canvas-grid-scroll" data-testid="canvas-grid-scroll" bind:this={scrollEl}>
    <div class="canvas-spacer" style="height: {totalSize}px" data-testid="canvas-spacer"></div>
    <canvas
      class="grid-canvas"
      data-testid="grid-canvas"
      bind:this={canvasEl}
      onclick={onClick}
      oncontextmenu={onContextMenu}
    ></canvas>
  </div>
</div>

<style>
  /* The spacer reserves the full scroll range; the canvas stays pinned to
     the viewport (sticky) and repaints only what is on screen. */
  .canvas-grid-scroll {
    position: relative;
  }
  .canvas-spacer {
    position: absolute;
    top: 0;
    left: 0;
    width: 1px;
  }
  .grid-canvas {
    position: sticky;
    top: 0;
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
