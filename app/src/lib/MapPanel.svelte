<!--
  Offline map panel (TASK-06-001, PLAN §4 "MapLibre GL JS + Protomaps
  PMTiles", ADR-0008). Renders the bundled low-zoom world basemap from the
  `basemap://` scheme handler — no tile server, no network access, ever
  (test-enforced). MapLibre loads lazily on first open so the main bundle
  (and every startup that never opens the map) stays small.

  The style comes from map-src.ts, the single place a URL enters the map.
  Without a resolved archive the panel shows its empty state instead of a
  blank canvas or an error. Pins/geotagging arrive with 06-002+.
-->
<script lang="ts">
  import { t } from "./i18n.svelte.js";
  import {
    MAP_INITIAL_CENTER,
    MAP_INITIAL_ZOOM,
    MAP_MAX_ZOOM,
    MAP_MIN_ZOOM,
    availableBasemap,
    mapStyle,
    type BasemapAvailability,
  } from "./map-src";

  let { open = $bindable(false) }: { open?: boolean } = $props();

  let status = $state<BasemapAvailability | null>(null);
  let checked = $state(false);
  let container = $state<HTMLDivElement | null>(null);
  /// The live maplibre map, cleaned up when the panel closes.
  let map: import("maplibre-gl").Map | null = null;

  $effect(() => {
    if (!open) return;
    status = null;
    checked = false;
    void (async () => {
      status = await availableBasemap();
      checked = true;
    })();
  });

  // Mount the map once the availability check passed and the container is
  // in the DOM. Re-runs close the previous map first (open/close cycles).
  $effect(() => {
    if (!open || !checked || !status?.available || !container) return;
    let cancelled = false;
    void (async () => {
      const { Map, NavigationControl } = await import("maplibre-gl");
      if (cancelled || !container) return;
      map = new Map({
        container,
        style: mapStyle(1) as Parameters<typeof Map.prototype.setStyle>[0],
        center: MAP_INITIAL_CENTER,
        zoom: MAP_INITIAL_ZOOM,
        minZoom: MAP_MIN_ZOOM,
        maxZoom: MAP_MAX_ZOOM,
        attributionControl: false,
        // The map canvas is keyboard-operable via its own controls; keep
        // default touch/drag gestures (a map is inherently interactive).
      });
      map.addControl(new NavigationControl({ showCompass: false }), "top-right");
    })();
    return () => {
      cancelled = true;
      map?.remove();
      map = null;
    };
  });
</script>

{#if open}
  <div class="map-backdrop" data-testid="map-backdrop">
    <div class="map-dialog" role="dialog" aria-label={t("map.title")}>
      <header>
        <h2>{t("map.title")}</h2>
        <button
          type="button"
          data-testid="map-close"
          onclick={() => (open = false)}
        >
          {t("map.close")}
        </button>
      </header>
      {#if checked && !status?.available}
        <p class="map-empty" data-testid="map-empty">{t("map.unavailable")}</p>
      {:else}
        <div class="map-stage">
          <div class="map-holder" bind:this={container} data-testid="map-holder"></div>
          <p class="map-attribution">{t("map.attribution")}</p>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .map-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: rgb(0 0 0 / 40%);
    display: grid;
    place-items: center;
  }
  .map-dialog {
    background: var(--fb-surface, #fff);
    color: var(--fb-text, #111);
    border-radius: 8px;
    width: min(60rem, 92vw);
    height: min(42rem, 88vh);
    display: grid;
    grid-template-rows: auto 1fr;
    padding: 0.75rem 1rem 1rem;
    box-shadow: 0 8px 32px rgb(0 0 0 / 30%);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.5rem;
  }
  h2 {
    margin: 0;
    font-size: 1.05rem;
  }
  .map-stage {
    position: relative;
    min-height: 0;
    border-radius: 6px;
    overflow: hidden;
    background: #f4efe9;
  }
  .map-holder {
    position: absolute;
    inset: 0;
  }
  .map-attribution {
    position: absolute;
    bottom: 0;
    right: 0;
    margin: 0;
    padding: 0.1rem 0.4rem;
    font-size: 0.65rem;
    background: rgb(255 255 255 / 75%);
    color: #333;
    z-index: 1;
  }
  .map-empty {
    place-self: center;
    color: var(--fb-text-muted, #666);
  }
</style>
