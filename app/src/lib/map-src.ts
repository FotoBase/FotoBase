// Offline basemap model (TASK-06-001, PLAN §4 "MapLibre GL JS + Protomaps
// PMTiles", ADR-0008). Pure decision + style-construction logic, free of
// Svelte/DOM — same pattern as video-src.ts / grid.ts.
//
// The no-network rule is structural, not behavioral: the inline style only
// ever references the `basemap://` custom scheme (served by the local
// PMTiles scheme handler), defines no glyphs and no sprite, and every
// layer reads vector tiles from that one source. `mapStyle` is the single
// place a URL enters the map, so the tests pin the invariant here.

/// The bundled world basemap's top zoom (tools/basemap/build.sh extracts
/// z0–z4 from the Protomaps build, ~6 MB per ADR-0008). Zooming past it
/// overzooms the z4 vector tiles — MapLibre re-projects them, no requests
/// leave the scheme.
export const BASEMAP_SOURCE_MAX_ZOOM = 4;

/// Panel zoom bounds: from the single world tile up to a useful overzoom
/// level for a bundled low-zoom basemap. Regional high-zoom bundles (the
/// opt-in download, ADR-0008) lift these later.
export const MAP_MIN_ZOOM = 0;
export const MAP_MAX_ZOOM = 12;

/// Initial view: the world, slightly past the single z0 tile.
export const MAP_INITIAL_CENTER: [number, number] = [10, 25];
export const MAP_INITIAL_ZOOM = 1.4;

/// What the backend `basemap_status` command reports.
export interface BasemapAvailability {
  available: boolean;
  /// Diagnostics only: where the archive resolved from.
  source: string | null;
}

/// Ask the backend whether the offline basemap resolved at startup. `null`
/// when there is no backend (browser dev) — the panel shows its empty
/// state instead of erroring.
export async function availableBasemap(): Promise<BasemapAvailability | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<BasemapAvailability>("basemap_status");
  } catch {
    return null;
  }
}

/// The tile URL template the style source fetches from. `version` busts
/// the WebView cache when a regional download replaces the archive (tiles
/// are immutable per archive generation, not across them).
export function basemapTileTemplate(version: number): string {
  return `basemap://{z}/{x}/{y}?v=${version}`;
}

/// The inline map style: Protomaps vector layers over the `basemap://`
/// source, painted in a fixed light palette (the map is a canvas — CSS
/// variables do not reach it). Deliberately no `glyphs` and no `sprite`:
/// both would be fetched over the network, which the map must never do.
/// Text labels arrive with the bundled-glyph work that accompanies the
/// regional basemap flow; the geometry basemap stays label-free for now.
export function mapStyle(version: number) {
  return {
    version: 8,
    name: "Fotobase offline basemap",
    sources: {
      basemap: {
        type: "vector",
        tiles: [basemapTileTemplate(version)],
        minzoom: 0,
        maxzoom: BASEMAP_SOURCE_MAX_ZOOM,
        // The archive holds the whole world; overzoom past z4 re-projects
        // the z4 tiles instead of requesting tiles the bundle lacks.
        bounds: [-180, -85.051129, 180, 85.051129],
        attribution:
          '<a href="https://www.openstreetmap.org/copyright" target="_blank" rel="noopener">© OpenStreetMap contributors</a>',
      },
    },
    layers: [
      {
        id: "background",
        type: "background",
        paint: { "background-color": "#f4efe9" },
      },
      {
        id: "landcover",
        type: "fill",
        source: "basemap",
        "source-layer": "landcover",
        filter: ["==", ["get", "kind"], "ice_shelf"],
        paint: { "fill-color": "#e8f0f8" },
      },
      {
        id: "landuse-urban",
        type: "fill",
        source: "basemap",
        "source-layer": "landuse",
        minzoom: 2,
        filter: [
          "in",
          ["get", "kind"],
          ["literal", ["urban", "residential", "commercial", "industrial"]],
        ],
        paint: { "fill-color": "#e8e2d8" },
      },
      {
        id: "water",
        type: "fill",
        source: "basemap",
        "source-layer": "water",
        filter: ["!=", ["get", "brunnel"], "tunnel"],
        paint: { "fill-color": "#a8c8e8" },
      },
      {
        id: "boundaries",
        type: "line",
        source: "basemap",
        "source-layer": "boundaries",
        filter: ["==", ["get", "kind"], "country"],
        paint: { "line-color": "#b0a090", "line-width": 0.7 },
      },
      {
        id: "roads",
        type: "line",
        source: "basemap",
        "source-layer": "roads",
        minzoom: 2,
        filter: ["in", ["get", "kind"], ["literal", ["highway", "major_road"]]],
        paint: { "line-color": "#d8c8a8", "line-width": 0.6 },
      },
    ],
  } as const;
}

// --- Clustered photo pins (TASK-06-002) -------------------------------------
//
// Pins cluster through MapLibre's clustered GeoJSON source (50k points is
// within its capability per the task's technical spec); the DOM-marker
// layer on top reads the queried features and draws cluster counts and
// `thumb://` photo thumbnails. Everything below is pure and IPC-free —
// hashes only, never image bytes (§3 rule 3).

/// One geotagged photo from the `map_pins` command (mirrors
/// `fotobase_catalog::MapPin`).
export interface MapPin {
  hash: string;
  latitude: number;
  longitude: number;
}

/// Cluster radius in screen pixels and the zoom past which every point is
/// its own pin. Pinned so the unit aggregation test (supercluster, the
/// same engine MapLibre uses) verifies the exact values the panel runs.
export const CLUSTER_RADIUS = 60;
export const CLUSTER_MAX_ZOOM = 10;

/// Upper bound on DOM markers per frame: MapLibre reports only on-screen
/// features, but a dense high-zoom view could still spawn thousands of
/// nodes. Past the cap the densest views simply show fewer markers —
/// clustering has already done the heavy lifting below it.
export const MARKER_LIMIT = 300;

/// Ask the backend for the geotagged pins. `null` without a backend
/// (browser dev) — the panel shows the basemap without pins, like the
/// availability check above.
export async function fetchPins(): Promise<MapPin[] | null> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<MapPin[]>("map_pins");
  } catch {
    return null;
  }
}

/// The pin dataset for the clustered GeoJSON source: one Point feature per
/// photo, keyed by content hash so the marker layer can build `thumb://`
/// URIs from the queried features alone.
export function pinsToGeoJson(pins: MapPin[]) {
  return {
    type: "FeatureCollection" as const,
    features: pins.map((p) => ({
      type: "Feature" as const,
      geometry: {
        type: "Point" as const,
        coordinates: [p.longitude, p.latitude],
      },
      properties: { hash: p.hash },
    })),
  };
}

/// The clustered source config the panel adds under the id `photos`.
export function photoClusterSource(pins: MapPin[]) {
  return {
    type: "geojson" as const,
    data: pinsToGeoJson(pins),
    cluster: true,
    clusterRadius: CLUSTER_RADIUS,
    clusterMaxZoom: CLUSTER_MAX_ZOOM,
  };
}

/// The `thumb://` URI for a pin's photo (same scheme the grid uses).
export function pinThumbUri(hash: string): string {
  return `thumb://${hash}`;
}

/// What the marker layer draws for one queried feature: a counted cluster
/// bubble or a thumbnail pin. Pure — the panel maps these onto DOM.
export type MarkerModel =
  | { kind: "cluster"; count: number; lon: number; lat: number }
  | { kind: "pin"; hash: string; thumb: string; lon: number; lat: number };

interface QueryFeature {
  geometry: { coordinates: number[] } | null;
  properties: {
    cluster?: boolean;
    point_count?: number;
    hash?: unknown;
  } | null;
}

/// Build the marker models from `map.querySourceFeatures("photos")`
/// output, capped at `limit`. Features without geometry or a usable
/// payload are skipped.
export function markerModel(
  features: QueryFeature[],
  limit: number,
): MarkerModel[] {
  const out: MarkerModel[] = [];
  for (const f of features) {
    if (out.length >= limit) break;
    const coords = f.geometry?.coordinates;
    if (!Array.isArray(coords) || coords.length !== 2) continue;
    const [lon, lat] = coords;
    if (f.properties?.cluster) {
      const count = f.properties.point_count;
      if (typeof count !== "number") continue;
      out.push({ kind: "cluster", count, lon, lat });
    } else if (typeof f.properties?.hash === "string" && f.properties.hash) {
      out.push({
        kind: "pin",
        hash: f.properties.hash,
        thumb: pinThumbUri(f.properties.hash),
        lon,
        lat,
      });
    }
  }
  return out;
}
