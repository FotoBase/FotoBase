// TASK-06-001 tests: the offline basemap model. The no-network rule is
// structural — the style only references the `basemap://` scheme, defines
// no glyphs/sprite (both would be network fetches), and bounds its zoom to
// what the bundle holds. `availableBasemap` degrades to null without a
// backend (browser dev), like every other IPC wrapper.

import { describe, expect, it, vi } from "vitest";

import {
  BASEMAP_SOURCE_MAX_ZOOM,
  MAP_INITIAL_ZOOM,
  MAP_MAX_ZOOM,
  availableBasemap,
  basemapTileTemplate,
  mapStyle,
} from "./map-src";

// TASK-06-002: clustered pins. The GeoJSON source clusters in MapLibre's
// engine; the pure helpers here build its dataset, pin its config and turn
// the queried features into DOM-marker models (cluster counts / `thumb://`
// pins). No image bytes ever enter these structures — hashes only.

import {
  CLUSTER_MAX_ZOOM,
  CLUSTER_RADIUS,
  MARKER_LIMIT,
  fetchPins,
  markerModel,
  photoClusterSource,
  pinThumbUri,
  pinsToGeoJson,
  type MapPin,
} from "./map-src";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

/// Every string reachable in a JSON-ish value.
function strings(value: unknown, out: string[] = []): string[] {
  if (typeof value === "string") out.push(value);
  else if (Array.isArray(value)) for (const v of value) strings(v, out);
  else if (value && typeof value === "object")
    for (const v of Object.values(value)) strings(v, out);
  return out;
}

describe("mapStyle — the no-network invariant", () => {
  const style = mapStyle(1);

  it("references only the basemap:// scheme, never http(s)", () => {
    for (const s of strings(style)) {
      expect(
        s.startsWith("basemap://") || !/^(https?):\/\//.test(s),
        `network URL in style: ${s}`,
      ).toBe(true);
    }
  });

  it("fetches tiles exclusively from the basemap scheme", () => {
    const source = style.sources.basemap;
    expect(source.tiles).toEqual([basemapTileTemplate(1)]);
    expect(source.tiles[0].startsWith("basemap://")).toBe(true);
  });

  it("declares no glyphs and no sprite (both would be fetched)", () => {
    expect("glyphs" in style).toBe(false);
    expect("sprite" in style).toBe(false);
  });

  it("caps the source at the bundle's top zoom", () => {
    expect(style.sources.basemap.maxzoom).toBe(BASEMAP_SOURCE_MAX_ZOOM);
  });

  it("covers the whole world (the bundle is the world basemap)", () => {
    expect(style.sources.basemap.bounds).toEqual([
      -180, -85.051129, 180, 85.051129,
    ]);
  });

  it("credits OpenStreetMap (ODbL attribution duty)", () => {
    const attribution = style.sources.basemap.attribution;
    expect(attribution).toContain("OpenStreetMap");
  });
});

describe("basemapTileTemplate", () => {
  it("carries the XYZ placeholders and the version bust", () => {
    expect(basemapTileTemplate(7)).toBe("basemap://{z}/{x}/{y}?v=7");
  });
});

describe("availableBasemap", () => {
  it("returns null without a backend (browser dev)", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockRejectedValue(new Error("no backend"));
    await expect(availableBasemap()).resolves.toBeNull();
  });

  it("returns the backend verdict", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue({
      available: true,
      source: "/bundled/world.pmtiles",
    });
    await expect(availableBasemap()).resolves.toEqual({
      available: true,
      source: "/bundled/world.pmtiles",
    });
  });
});

describe("initial view constants", () => {
  it("start inside the bundle's zoom range", () => {
    expect(MAP_INITIAL_ZOOM).toBeGreaterThanOrEqual(0);
    expect(MAP_INITIAL_ZOOM).toBeLessThanOrEqual(BASEMAP_SOURCE_MAX_ZOOM);
  });
});

// TASK-06-002: clustered pins. The GeoJSON source clusters in MapLibre's
// engine; the pure helpers here build its dataset, pin its config and turn
// the queried features into DOM-marker models (cluster counts / `thumb://`
// pins). No image bytes ever enter these structures — hashes only.

const PIN = (i: number, lat = 52.5, lon = 13.4): MapPin => ({
  hash: `hash-${i.toString().padStart(4, "0")}`,
  latitude: lat,
  longitude: lon,
});

describe("pinsToGeoJson", () => {
  it("maps every pin to a Point feature carrying its hash", () => {
    const fc = pinsToGeoJson([PIN(1, 10, 20), PIN(2, -10, -20)]);
    expect(fc.type).toBe("FeatureCollection");
    expect(fc.features).toHaveLength(2);
    const [a] = fc.features;
    expect(a.geometry).toEqual({ type: "Point", coordinates: [20, 10] });
    expect(a.properties).toEqual({ hash: "hash-0001" });
  });

  it("carries no image bytes — hashes only (§3 rule 3)", () => {
    const fc = pinsToGeoJson([PIN(1)]);
    expect(JSON.stringify(fc)).not.toMatch(/data:image|base64/);
  });
});

describe("photoClusterSource", () => {
  it("clusters the pin dataset with the pinned engine config", () => {
    const source = photoClusterSource([PIN(1), PIN(2)]);
    expect(source.type).toBe("geojson");
    expect(source.cluster).toBe(true);
    expect(source.clusterRadius).toBe(CLUSTER_RADIUS);
    expect(source.clusterMaxZoom).toBe(CLUSTER_MAX_ZOOM);
    expect(source.data.features).toHaveLength(2);
  });

  it("stays inside the panel's zoom bounds", () => {
    expect(CLUSTER_MAX_ZOOM).toBeLessThanOrEqual(MAP_MAX_ZOOM);
  });
});

describe("fetchPins", () => {
  it("returns null without a backend (browser dev), like every IPC wrapper", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockRejectedValue(new Error("no backend"));
    await expect(fetchPins()).resolves.toBeNull();
  });

  it("returns the backend's pins", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue([PIN(1)]);
    await expect(fetchPins()).resolves.toEqual([PIN(1)]);
  });
});

describe("pinThumbUri", () => {
  it("is a thumb:// URI keyed by the content hash", () => {
    expect(pinThumbUri("abc")).toBe("thumb://abc");
  });
});

describe("markerModel", () => {
  it("turns cluster features into counted cluster markers", () => {
    const model = markerModel(
      [
        {
          geometry: { coordinates: [13.4, 52.5] },
          properties: { cluster: true, point_count: 42 },
        } as never,
      ],
      10,
    );
    expect(model).toEqual([
      { kind: "cluster", count: 42, lon: 13.4, lat: 52.5 },
    ]);
  });

  it("turns unclustered features into thumbnail pins", () => {
    const model = markerModel(
      [
        {
          geometry: { coordinates: [1, 2] },
          properties: { hash: "h1" },
        } as never,
      ],
      10,
    );
    expect(model).toEqual([
      { kind: "pin", hash: "h1", thumb: "thumb://h1", lon: 1, lat: 2 },
    ]);
  });

  it("caps the DOM marker count (a 50k library must not flood the page)", () => {
    const feats = Array.from({ length: 500 }, (_, i) => ({
      geometry: { coordinates: [i, i] },
      properties: { cluster: true, point_count: 1 },
    })) as never[];
    expect(markerModel(feats, MARKER_LIMIT)).toHaveLength(MARKER_LIMIT);
  });

  it("skips features without a usable geometry or payload", () => {
    const model = markerModel(
      [
        { geometry: null, properties: { cluster: true, point_count: 1 } },
        { geometry: { coordinates: [0, 0] }, properties: {} },
      ] as never,
      10,
    );
    expect(model).toEqual([]);
  });
});

describe("cluster aggregation at a zoom boundary", () => {
  // The unit gate the task's testing criteria demand: with the pinned
  // engine config, cluster counts must add up to the photo total at every
  // zoom, and past the cluster-max zoom every point is its own pin.
  // supercluster is MapLibre's own clustering engine, mirrored here in a
  // WebGL-free unit test.
  it("counts match photo totals at a zoom boundary, and resolve fully past CLUSTER_MAX_ZOOM", async () => {
    const Supercluster = (await import("supercluster")).default;
    const pins: MapPin[] = Array.from({ length: 500 }, (_, i) =>
      PIN(i, 40 + (i % 20) * 0.5, 10 + Math.floor(i / 20) * 0.5),
    );
    const index = new Supercluster<{ hash: string }>({
      radius: CLUSTER_RADIUS,
      maxZoom: CLUSTER_MAX_ZOOM,
    });
    index.load(pinsToGeoJson(pins).features as never);

    for (const zoom of [0, 4, CLUSTER_MAX_ZOOM]) {
      const clusters = index.getClusters([-180, -85, 180, 85], zoom) as Array<{
        properties: { cluster?: boolean; point_count?: number; hash?: string };
      }>;
      const total = clusters.reduce(
        (sum, f) =>
          sum + (f.properties.cluster ? (f.properties.point_count ?? 0) : 1),
        0,
      );
      expect(total).toBe(pins.length);
    }
    const resolved = index.getClusters(
      [-180, -85, 180, 85],
      CLUSTER_MAX_ZOOM + 1,
    ) as Array<{
      properties: { cluster?: boolean };
    }>;
    expect(resolved).toHaveLength(pins.length);
    expect(resolved.every((f) => !f.properties.cluster)).toBe(true);
  });
});
