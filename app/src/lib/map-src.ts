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
        filter: ["in", ["get", "kind"], ["literal", ["urban", "residential", "commercial", "industrial"]]],
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
