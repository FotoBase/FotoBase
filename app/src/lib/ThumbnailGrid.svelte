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
    Virtualizer,
    elementScroll,
    observeElementOffset,
    observeElementRect,
    type VirtualItem,
  } from "@tanstack/virtual-core";
  import { t } from "./i18n.svelte.js";
  import { columnCount, debounce, rowItems, thumbUri, visibleHashes, GRID_GAP, type GridItem } from "./grid";

  interface Props {
    items?: GridItem[];
    /// Thumbnail edge in px; the zoom slider drives it live.
    thumbSize?: number;
    overscan?: number;
    /// Called (debounced) with the unique hashes on screen.
    onvisible?: (hashes: string[]) => void;
  }

  let { items = [], thumbSize = 160, overscan = 2, onvisible = () => {} }: Props = $props();

  /// Zoom range: 64 = wall of small tiles, 512 = near-preview.
  const ZOOM_MIN = 64;
  const ZOOM_MAX = 512;
  const ZOOM_STEP = 8;
  const REPORT_DEBOUNCE_MS = 100;

  let zoom = $state(thumbSize);
  let scrollEl = $state<HTMLDivElement | null>(null);
  let virtualRows = $state<VirtualItem[]>([]);
  let totalSize = $state(0);
  let columns = $state(1);

  const reportVisible = debounce((hashes: string[]) => onvisible(hashes), REPORT_DEBOUNCE_MS);

  $effect(() => {
    const el = scrollEl;
    if (!el) return;
    // Read every reactive input so the effect (and with it the virtualizer)
    // rebuilds when the zoom level or the collection changes.
    void zoom;
    void items.length;

    const cols = columnCount(el.clientWidth, zoom, GRID_GAP);
    columns = cols;
    const rowCount = Math.ceil(items.length / cols);

    const virtualizer = new Virtualizer({
      getScrollElement: () => el,
      count: rowCount,
      estimateSize: () => zoom + GRID_GAP,
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
      bind:value={zoom}
    />
  </div>
  <div class="grid-scroll" data-testid="grid-scroll" bind:this={scrollEl}>
    <div class="grid-inner" style="height: {totalSize}px">
      {#each virtualRows as row (row.key)}
        <div
          class="grid-row"
          style="transform: translateY({row.start}px); height: {row.size}px; grid-template-columns: repeat({columns}, var(--thumb-size))"
        >
          {#each rowItems(items, columns, row.index) as item (item.id)}
            <div class="grid-cell" data-testid="grid-cell" style="--thumb-size: {zoom}px">
              {#if thumbUri(item.hash)}
                <img
                  src={thumbUri(item.hash) ?? ""}
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
