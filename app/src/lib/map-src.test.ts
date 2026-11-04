// TASK-06-001 tests: the offline basemap model. The no-network rule is
// structural — the style only references the `basemap://` scheme, defines
// no glyphs/sprite (both would be network fetches), and bounds its zoom to
// what the bundle holds. `availableBasemap` degrades to null without a
// backend (browser dev), like every other IPC wrapper.

import { describe, expect, it, vi } from "vitest";

import {
  BASEMAP_SOURCE_MAX_ZOOM,
  MAP_INITIAL_ZOOM,
  availableBasemap,
  basemapTileTemplate,
  mapStyle,
} from "./map-src";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

/// Every string reachable in a JSON-ish value.
function strings(value: unknown, out: string[] = []): string[] {
  if (typeof value === "string") out.push(value);
  else if (Array.isArray(value)) for (const v of value) strings(v, out);
  else if (value && typeof value === "object") for (const v of Object.values(value)) strings(v, out);
  return out;
}

describe("mapStyle — the no-network invariant", () => {
  const style = mapStyle(1);

  it("references only the basemap:// scheme, never http(s)", () => {
    for (const s of strings(style)) {
      expect(s.startsWith("basemap://") || !/^(https?):\/\//.test(s), `network URL in style: ${s}`).toBe(true);
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
    expect(style.sources.basemap.bounds).toEqual([-180, -85.051129, 180, 85.051129]);
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
    vi.mocked(invoke).mockResolvedValue({ available: true, source: "/bundled/world.pmtiles" });
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
