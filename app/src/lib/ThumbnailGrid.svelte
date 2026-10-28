<!--
  Virtualized DOM thumbnail grid (TASK-01-013, PLAN §4: TanStack Virtual).

  Renders only the rows near the scroll viewport, so the DOM stays bounded
  for 100k+ images. Cells size from the zoom slider purely in CSS — zooming
  never re-fetches; the cache already holds the thumbnails. Srcs are
  `thumb://` URIs only (PLAN §3 rule 3): no image bytes cross IPC. The
  visible range is reported (debounced) so the backend can bump those
  thumbnails' render jobs to visible-first priority (PLAN §3 rule 4).
-->
<script lang="ts">
  import {
    elementScroll,
    observeElementOffset,
    observeElementRect,
    Virtualizer,
    type VirtualItem,
  } from "@tanstack/virtual-core";
  import { untrack } from "svelte";
  import { columnCount, debounce, GRID_GAP, nextCell, revealScrollTop, rowItems, thumbSrcset, thumbUri, visibleHashes, type GridItem } from "./grid";
  import { gridState, ZOOM_MAX, ZOOM_MIN, ZOOM_STEP } from "./grid-state.svelte.js";
  import { t } from "./i18n.svelte.js";

  interface Props {
    items?: GridItem[];
    overscan?: number;
    /// Called (debounced) with the unique hashes on screen.
    onvisible?: (hashes: string[]) => void;
    /// Called with the photo the user opened (double-click or Enter —
    /// TASK-01-019): the shell mounts the full-view viewer over the list.
    onopen?: (item: GridItem) => void;
    /// Called with the photo the user right-clicked (TASK-01-021): the shell
    /// opens its per-photo context menu at the pointer.
    oncontextmenu?: (e: { x: number; y: number; item: GridItem }) => void;
  }

  let {
    items = [],
    overscan = 2,
    onvisible = () => {},
    onopen = () => {},
    oncontextmenu = () => {},
  }: Props = $props();

  const REPORT_DEBOUNCE_MS = 100;

  // Keyboard grid navigation (TASK-01-024): arrows move focus and selection
  // to the neighbor cell (pure math in `nextCell`). The target row is
  // virtually always rendered (overscan ≥ 1 on every side); when it is not,
  // the selection still moves and focus stays put — the next arrow press
  // re-targets from the selection, so nothing is lost.
  function onCellKeydown(e: KeyboardEvent, index: number, item: GridItem): void {
    if (e.key === "Enter") {
      onopen(item);
      return;
    }
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight" && e.key !== "ArrowUp" && e.key !== "ArrowDown") {
      return;
    }
    e.preventDefault();
    const target = nextCell(index, columns, items.length, e.key);
    if (target === null) return;
    gridState.selection = new Set([items[target].id]);
    scrollEl?.querySelector<HTMLElement>(`[data-cell-index="${target}"]`)?.focus();
  }

  let scrollEl = $state<HTMLDivElement | null>(null);
  let virtualRows = $state<VirtualItem[]>([]);
  let totalSize = $state(0);
  let columns = $state(1);

  const reportVisible = debounce((hashes: string[]) => onvisible(hashes), REPORT_DEBOUNCE_MS);

  // Resume where the last renderer (or session) left off. Mount-only: the
  // store's scrollTop is read untracked — the virtualizer writes it on every
  // scroll, and reading it reactively here would re-run this effect (and
  // with it the whole virtualizer) on every scroll event.
  $effect(() => {
    const el = scrollEl;
    if (!el) return;
    el.scrollTop = untrack(() => gridState.scrollTop);
  });

  // Tray jump (TASK-03-010): scroll the requested photo's row into view and
  // select it. Stays pending until the photo is among the shown items.
  $effect(() => {
    const request = gridState.revealRequest;
    const el = scrollEl;
    if (!request || !el) return;
    const index = items.findIndex((i) => i.id === request.id);
    if (index < 0) return;
    const cols = columnCount(el.clientWidth, gridState.thumbSize, GRID_GAP);
    el.scrollTop = revealScrollTop(index, cols, gridState.thumbSize);
    gridState.selection = new Set([request.id]);
    gridState.revealRequest = null;
  });

  $effect(() => {
    const el = scrollEl;
    if (!el) return;
    // Read every reactive input so the effect (and with it the virtualizer)
    // rebuilds when the zoom level or the collection changes. Zoom lives in
    // the shared grid store (TASK-01-014) so both renderers stay in sync.
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
        const rows = inst.getVirtualItems();
        virtualRows = rows;
        totalSize = inst.getTotalSize();
        gridState.scrollTop = inst.scrollOffset ?? 0;
        // Visible range moved (scroll, resize): feed the queue's
        // visible-first priority (PLAN §3 rule 4), debounced.
        reportVisible(visibleHashes(items, cols, rows.map((r) => r.index)));
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
  <div class="grid-scroll" data-testid="grid-scroll" bind:this={scrollEl}>
    <div class="grid-inner" style="height: {totalSize}px">
      {#each virtualRows as row (row.key)}
        <div
          class="grid-row"
          style="transform: translateY({row.start}px); height: {row.size}px; grid-template-columns: repeat({columns}, var(--thumb-size))"
        >
          {#each rowItems(items, columns, row.index) as item, localIdx (item.id)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class="grid-cell"
              data-testid="grid-cell"
              data-hidden={item.hidden ? "true" : undefined}
              data-cell-index={row.index * columns + localIdx}
              style="--thumb-size: {gridState.thumbSize}px"
              tabindex="0"
              ondblclick={() => onopen(item)}
              oncontextmenu={(e) => {
                e.preventDefault();
                oncontextmenu({ x: e.clientX, y: e.clientY, item });
              }}
              onkeydown={(e) => onCellKeydown(e, row.index * columns + localIdx, item)}
            >
              {#if thumbUri(item.hash)}
                {@const version = gridState.thumbVersions[item.hash] ?? 0}
                <img
                  src={thumbUri(item.hash, version) ?? ""}
                  srcset={thumbSrcset(item.hash, window.devicePixelRatio, version) ?? undefined}
                  alt={item.filename}
                  loading="lazy"
                  draggable="false"
                />
              {:else}
                <div class="grid-cell-placeholder" data-testid="grid-placeholder"></div>
              {/if}
            </div>
          {/each}
        </div>
      {/each}
    </div>
  </div>
</div>
