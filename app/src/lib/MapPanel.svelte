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
  import { debounce } from "./grid";
  import {
    MAP_INITIAL_CENTER,
    MAP_INITIAL_ZOOM,
    MAP_MAX_ZOOM,
    MAP_MIN_ZOOM,
    MARKER_LIMIT,
    availableBasemap,
    fetchFilteredPins,
    mapStyle,
    markerModel,
    photoClusterSource,
    pinLayers,
    pinsToGeoJson,
    type BasemapAvailability,
    type MarkerModel,
  } from "./map-src";
  import type { LibraryView } from "./search";
  import type { TrayPhoto } from "./tray.svelte";
  import { acceptsPhotoDrag, geotagPhotos, readPhotoDrop } from "./geotag-drag";

  let {
    open = $bindable(false),
    view = null,
    onopenphoto = () => {},
    ontray = () => {},
  }: {
    open?: boolean;
    view?: LibraryView | null;
    /// A plain pin click/tap: open that photo in the viewer (TASK-06-004).
    onopenphoto?: (photo: TrayPhoto) => void;
    /// The tray action: the selected pins feed the Photo Tray.
    ontray?: (photos: TrayPhoto[]) => void;
  } = $props();

    /// Pins the user selected (modifier click / long-press); the tray action
    /// hands them to the Photo Tray and clears the set.
    let selected = $state<Map<number, TrayPhoto>>(new Map());

  let status = $state<BasemapAvailability | null>(null);
  let checked = $state(false);
  let container = $state<HTMLDivElement | null>(null);
  /// The live maplibre map, cleaned up when the panel closes.
  let map: import("maplibre-gl").Map | null = null;
  /// The DOM markers currently on the map, cleared and rebuilt on every
  /// move/data change (clustering re-runs server-side in MapLibre's
  /// engine; the DOM layer only mirrors what is on screen).
  let markers: import("maplibre-gl").Marker[] = [];
  /// The clustered source once added; view changes update it in place
  /// instead of rebuilding source + layers.
  let source: import("maplibre-gl").GeoJSONSource | null = null;
  /// True once the map exists — the view-change effect waits for it.
  let ready = $state(false);
  /// The lazily-imported Marker constructor, captured by the mount effect
  /// so draw() (called from the debounced reload) can build markers.
  let markerCtor: (typeof import("maplibre-gl"))["Marker"] | null = null;
  /// The view the pending debounced re-query uses (read at fire time).
  let pendingView: LibraryView | null = null;

  const reloadPins = debounce(() => {
    const v = pendingView ?? undefined;
    void (async () => {
      const pins = await fetchFilteredPins(v);
      if (!map || !pins) return;
      if (source) {
        source.setData(pinsToGeoJson(pins));
        draw();
      } else {
        addPinSource(pins);
      }
    })();
  }, 250);

  function markerElement(model: MarkerModel): HTMLElement {
    const el = document.createElement("div");
    if (model.kind === "cluster") {
      el.className = "map-cluster";
      el.dataset.testid = "map-cluster";
      el.textContent = model.count > 999 ? "999+" : String(model.count);
      // A cluster click zooms in to resolve it into its photos.
      el.addEventListener("click", (e) => {
        e.stopPropagation();
        zoomToCluster(model.lon, model.lat);
      });
    } else {
      el.className = "map-pin";
      el.dataset.testid = "map-pin";
      if (model.id !== undefined) el.dataset.photoId = String(model.id);
      if (selected.has(model.id ?? -1)) el.classList.add("selected");
      // Plain click/tap opens the photo; a modifier click selects. The
      // long-press fallback covers touch, where there is no modifier key.
      let pressTimer: ReturnType<typeof setTimeout> | null = null;
      let longPressed = false;
      el.addEventListener("pointerdown", (e) => {
        if (e.pointerType === "mouse") return;
        longPressed = false;
        pressTimer = setTimeout(() => {
          longPressed = true;
          togglePin(model);
        }, 500);
      });
      const cancel = (): void => {
        if (pressTimer !== null) clearTimeout(pressTimer);
        pressTimer = null;
      };
      el.addEventListener("pointerup", cancel);
      el.addEventListener("pointercancel", cancel);
      el.addEventListener("pointerleave", cancel);
      el.addEventListener("click", (e) => {
        e.stopPropagation();
        if (longPressed) {
          longPressed = false;
          return;
        }
        if (e.ctrlKey || e.metaKey || e.shiftKey) togglePin(model);
        else if (model.id !== undefined)
          onopenphoto({ id: model.id, filename: model.filename ?? "", hash: model.hash });
      });
      const img = document.createElement("img");
      img.src = model.thumb;
      img.alt = "";
      el.appendChild(img);
    }
    return el;
  }

  function togglePin(model: MarkerModel & { kind: "pin" }): void {
    if (model.id === undefined) return;
    selected = new Map(selected);
    const nowSelected = !selected.has(model.id);
    if (nowSelected) selected.set(model.id, { id: model.id, filename: model.filename ?? "", hash: model.hash });
    else selected.delete(model.id);
    // Mirror the state onto the live marker element (draw() would rebuild
    // it and orphan any open-press timers).
    const id = String(model.id);
    for (const m of markers) {
      const el = m.getElement();
      if (el?.dataset.photoId === id) el.classList.toggle("selected", nowSelected);
    }
  }

  function zoomToCluster(lon: number, lat: number): void {
    if (!map) return;
    map.easeTo({ center: [lon, lat], zoom: Math.min(map.getZoom() + 2, MAP_MAX_ZOOM) });
  }

  function addToTray(): void {
    if (selected.size === 0) return;
    ontray([...selected.values()]);
    selected = new Map();
  }

  /// Drag-to-geotag (TASK-06-005, PLAN §2H): only Fotobase photo drags are
  /// accepted; everything else (files, text) falls through. `map.unproject`
  /// turns the drop point's container coordinates into decimal degrees.
  function dragOver(e: DragEvent): void {
    if (!e.dataTransfer || !acceptsPhotoDrag(e.dataTransfer)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
  }

  let geotaggedCount = $state<number | null>(null);

  async function drop(e: DragEvent): Promise<void> {
    geotaggedCount = null;
    if (!e.dataTransfer || !map) return;
    const ids = readPhotoDrop(e.dataTransfer);
    if (!ids || ids.length === 0) return;
    e.preventDefault();
    const rect = container?.getBoundingClientRect();
    const x = e.clientX - (rect?.left ?? 0);
    const y = e.clientY - (rect?.top ?? 0);
    const lngLat = map.unproject([x, y]);
    try {
      await geotagPhotos(ids, lngLat.lat, lngLat.lng);
      geotaggedCount = ids.length;
    } catch {
      // The backend refusal (e.g. a vanished sidecar) leaves the map as it
      // was; the drop simply counts as not delivered.
    }
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
      await import("./maplibre-worker");
      if (cancelled || !container) return;
      markerCtor = Marker;
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
      // leaves the basemap without them. addSource/addLayer throw before
      // the style is loaded — addPinSource defers until it is.
      pendingView = view;
      reloadPins();
      ready = true;
    })();
    return () => {
      cancelled = true;
      ready = false;
      source = null;
      selected = new Map();
      for (const m of markers) m.remove();
      markers = [];
      map?.remove();
      map = null;
    };
  });

  // Live updates (TASK-06-003): a search/filter change re-queries the
  // filtered pin set, debounced so typing does not spam IPC. Fires once the
  // map is ready; the mount path above already loaded the initial view.
  $effect(() => {
    if (!ready) return;
    pendingView = view;
    reloadPins();
  });

  /// Rebuild the DOM markers from the queried (on-screen) features.
  function draw(): void {
    if (!map) return;
    for (const m of markers) m.remove();
    markers = [];
    for (const model of markerModel(
      map.querySourceFeatures("photos") as unknown as Parameters<typeof markerModel>[0],
      MARKER_LIMIT,
    )) {
      if (!markerCtor) return;
      markers.push(
        new markerCtor({ element: markerElement(model) }).setLngLat([model.lon, model.lat]).addTo(map),
      );
    }
  }

  /// Add the clustered source + layers for `pins`, deferring until the
  /// style has loaded (addSource throws before that).
  function addPinSource(pins: import("./map-src").MapPin[]): void {
    if (!map) return;
    const usePins = (): void => {
      if (!map) return;
      map.addSource("photos", photoClusterSource(pins));
      source = map.getSource("photos") as import("maplibre-gl").GeoJSONSource;
      // Reference the source from layers so MapLibre actually loads its
      // data (querySourceFeatures is only reliable for used sources).
      for (const layer of pinLayers()) map.addLayer(layer as never);
      map.on("moveend", draw);
      map.on("sourcedata", (e: { sourceId?: string; isSourceLoaded?: boolean }) => {
        if (e.sourceId === "photos" && e.isSourceLoaded) draw();
      });
      draw();
    };
    if (map.isStyleLoaded()) usePins();
    else map.once("load", usePins);
  }
</script>

{#if open}
  <div class="map-backdrop" data-testid="map-backdrop">
    <div class="map-dialog" role="dialog" aria-label={t("map.title")}>
      <header>
        <h2>{t("map.title")}</h2>
        {#if selected.size > 0}
          <button
            type="button"
            data-testid="map-add-to-tray"
            onclick={addToTray}
          >
            {t("map.addToTray", { count: selected.size })}
          </button>
        {/if}
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
          <div
            class="map-holder"
            bind:this={container}
            data-testid="map-holder"
            ondragover={dragOver}
            ondrop={drop}
          ></div>
          {#if geotaggedCount !== null}
            <p class="map-geotag-note" data-testid="map-geotag-note">
              {t("map.geotagged", { count: geotaggedCount })}
            </p>
          {:else}
            <p class="map-geotag-note map-geotag-hint" data-testid="map-geotag-hint">
              {t("map.dropToGeotag")}
            </p>
          {/if}
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
  .map-geotag-note {
    position: absolute;
    top: 0.5rem;
    left: 50%;
    transform: translateX(-50%);
    margin: 0;
    padding: 0.25rem 0.6rem;
    font-size: 0.75rem;
    background: rgb(255 255 255 / 85%);
    color: #333;
    border-radius: 4px;
    z-index: 1;
    pointer-events: none;
  }
  .map-geotag-hint {
    color: var(--fb-text-muted, #666);
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
    pointer-events: auto;
    cursor: pointer;
  }
  .map-stage :global(.map-pin.selected) {
    outline: 2px solid var(--fb-accent, #3567b0);
    outline-offset: 1px;
  }
  .map-stage :global(.map-pin img) {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
</style>
