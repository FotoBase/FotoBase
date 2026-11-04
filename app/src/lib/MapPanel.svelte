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
    MARKER_LIMIT,
    availableBasemap,
    fetchPins,
    mapStyle,
    markerModel,
    photoClusterSource,
    pinLayers,
    type BasemapAvailability,
    type MarkerModel,
  } from "./map-src";

  let { open = $bindable(false) }: { open?: boolean } = $props();

  let status = $state<BasemapAvailability | null>(null);
  let checked = $state(false);
  let container = $state<HTMLDivElement | null>(null);
  /// The live maplibre map, cleaned up when the panel closes.
  let map: import("maplibre-gl").Map | null = null;
  /// The DOM markers currently on the map, cleared and rebuilt on every
  /// move/data change (clustering re-runs server-side in MapLibre's
  /// engine; the DOM layer only mirrors what is on screen).
  let markers: import("maplibre-gl").Marker[] = [];

  function markerElement(model: MarkerModel): HTMLElement {
    const el = document.createElement("div");
    if (model.kind === "cluster") {
      el.className = "map-cluster";
      el.dataset.testid = "map-cluster";
      el.textContent = model.count > 999 ? "999+" : String(model.count);
    } else {
      el.className = "map-pin";
      el.dataset.testid = "map-pin";
      const img = document.createElement("img");
      img.src = model.thumb;
      img.alt = "";
      el.appendChild(img);
    }
    return el;
  }

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
      const { Map, Marker, NavigationControl } = await import("maplibre-gl");
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
      // E2e surface: the specs drive the map through its public API
      // (e2e/specs/map-*.e2e.ts) because synthetic wheel/key events do not
      // reach MapLibre under WebKitWebDriver. Not used by the app itself.
      (container as unknown as { __fbMap?: unknown }).__fbMap = map;
      // Pins arrive after the map exists; a failed fetch (browser dev)
      // leaves the basemap without them. The style may still be loading —
      // addSource/addLayer throw before it is — so defer until it is.
      const pins = await fetchPins();
      if (cancelled || !map) return;
      if (pins && pins.length > 0) {
        const usePins = (): void => {
          if (!map) return;
          map.addSource("photos", photoClusterSource(pins));
          // Reference the source from layers so MapLibre actually loads
          // its data (querySourceFeatures is only reliable for used
          // sources).
          for (const layer of pinLayers()) map.addLayer(layer as never);
          const draw = (): void => {
            if (!map) return;
            for (const m of markers) m.remove();
            markers = [];
            for (const model of markerModel(
              map.querySourceFeatures("photos") as unknown as Parameters<typeof markerModel>[0],
              MARKER_LIMIT,
            )) {
              markers.push(
                new Marker({ element: markerElement(model) }).setLngLat([model.lon, model.lat]).addTo(map),
              );
            }
          };
          map.on("moveend", draw);
          map.on("sourcedata", (e: { sourceId?: string; isSourceLoaded?: boolean }) => {
            if (e.sourceId === "photos" && e.isSourceLoaded) draw();
          });
          draw();
        };
        if (map.isStyleLoaded()) usePins();
        else map.once("load", usePins);
      }
    })();
    return () => {
      cancelled = true;
      for (const m of markers) m.remove();
      markers = [];
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
  /* Cluster and pin markers are DOM elements created by the marker layer
     at runtime; Svelte cannot see them in the template, hence :global. */
  .map-stage :global(.map-cluster) {
    display: grid;
    place-items: center;
    min-width: 2rem;
    height: 2rem;
    padding: 0 0.4rem;
    border-radius: 999px;
    background: var(--fb-accent, #3567b0);
    color: #fff;
    font-size: 0.75rem;
    font-weight: 600;
    box-shadow: 0 1px 4px rgb(0 0 0 / 40%);
    pointer-events: none;
  }
  .map-stage :global(.map-pin) {
    width: 2.5rem;
    height: 2.5rem;
    border-radius: 4px;
    overflow: hidden;
    box-shadow: 0 1px 4px rgb(0 0 0 / 40%);
    pointer-events: none;
  }
  .map-stage :global(.map-pin img) {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
</style>
