// TASK-06-001 component tests: the map panel. The availability check gates
// everything — no archive, no canvas, just the empty state. With an
// archive the map is constructed against the `basemap://` style (the
// no-network invariant itself is pinned in map-src.test.ts) and removed on
// close. maplibre-gl is mocked: jsdom has no WebGL and the real
// constructor would only test the mock anyway.

import { cleanup, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import MapPanel from "./MapPanel.svelte";
import {
  availableBasemap,
  mapStyle,
  pinLayers,
  pinsToGeoJson,
} from "./map-src";
import { Map as MapMock, Marker, NavigationControl } from "maplibre-gl";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("./map-src", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./map-src")>()),
  availableBasemap: vi.fn(),
}));
vi.mock("maplibre-gl", () => ({
  Map: vi.fn().mockImplementation(function () {
    return {
      addControl: vi.fn(),
      remove: vi.fn(),
      addSource: vi.fn(),
      addLayer: vi.fn(),
      querySourceFeatures: vi.fn(() => []),
      on: vi.fn(),
      isStyleLoaded: vi.fn(() => true),
    };
  }),
  Marker: vi.fn().mockImplementation(function () {
    return {
      setLngLat: vi.fn().mockReturnThis(),
      addTo: vi.fn().mockReturnThis(),
      remove: vi.fn(),
    };
  }),
  NavigationControl: vi.fn(),
}));

const PINS = [
  { hash: "h1", latitude: 52.5, longitude: 13.4 },
  { hash: "h2", latitude: -33.9, longitude: 151.2 },
];

function mockStatus(available: boolean): void {
  vi.mocked(availableBasemap).mockResolvedValue(
    available ? { available, source: "/bundle" } : { available, source: null },
  );
}

function mapInstance(): {
  addControl: ReturnType<typeof vi.fn>;
  addSource: ReturnType<typeof vi.fn>;
  addLayer: ReturnType<typeof vi.fn>;
  querySourceFeatures: ReturnType<typeof vi.fn>;
  on: ReturnType<typeof vi.fn>;
  remove: ReturnType<typeof vi.fn>;
} {
  return vi.mocked(MapMock).mock.results[0].value;
}

/// Fire the registered handler for an event.
function fire(
  map: ReturnType<typeof mapInstance>,
  event: string,
  arg?: unknown,
): void {
  for (const call of map.on.mock.calls) {
    if (call[0] === event) (call[1] as (a?: unknown) => void)(arg);
  }
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("MapPanel", () => {
  it("shows the empty state when no basemap archive resolved", async () => {
    mockStatus(false);
    render(MapPanel, { props: { open: true } });
    await waitFor(() => expect(screen.getByTestId("map-empty")).toBeTruthy());
    expect(MapMock).not.toHaveBeenCalled();
  });

  it("constructs the map from the offline style when the archive resolved", async () => {
    mockStatus(true);
    render(MapPanel, { props: { open: true } });
    await waitFor(() => expect(MapMock).toHaveBeenCalled());
    const options = vi.mocked(MapMock).mock.calls[0][0];
    expect(options.style).toEqual(mapStyle(1));
    expect(JSON.stringify(options.style)).toContain("basemap://");
    expect(options.minZoom).toBe(0);
  });

  it("tears the map down when the panel closes", async () => {
    mockStatus(true);
    const instance = {
      addControl: vi.fn(),
      remove: vi.fn(),
      addSource: vi.fn(),
      addLayer: vi.fn(),
      querySourceFeatures: vi.fn(() => []),
      on: vi.fn(),
      isStyleLoaded: vi.fn(() => true),
    };
    (
      MapMock as unknown as { mockImplementation: (f: () => unknown) => void }
    ).mockImplementation(function () {
      return instance as unknown as import("maplibre-gl").Map;
    });
    const { rerender } = render(MapPanel, { props: { open: true } });
    await waitFor(() => expect(instance.addControl).toHaveBeenCalled());
    rerender({ open: false });
    await waitFor(() => expect(instance.remove).toHaveBeenCalled());
    expect(NavigationControl).toHaveBeenCalled();
  });
});

// TASK-06-002: clustered pins. The panel fetches the pins over IPC
// (hashes + coordinates only), feeds the clustered GeoJSON source, and
// renders the queried features as DOM markers — cluster counts and
// `thumb://` thumbnails. Without a backend it still opens the basemap,
// just without pins.
describe("MapPanel — clustered pins", () => {
  beforeEach(() => {
    mockStatus(true);
  });

  it("adds the clustered source with the fetched pins", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue(PINS);
    render(MapPanel, { props: { open: true } });
    await waitFor(() => expect(mapInstance().addSource).toHaveBeenCalled());
    const [id, source] = mapInstance().addSource.mock.calls[0];
    expect(id).toBe("photos");
    expect(source.cluster).toBe(true);
    expect(source.data).toEqual(pinsToGeoJson(PINS));
  });

  it("adds the pin layers so the clustered source actually loads", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue(PINS);
    render(MapPanel, { props: { open: true } });
    await waitFor(() => expect(mapInstance().addLayer).toHaveBeenCalled());
    expect(
      mapInstance().addLayer.mock.calls.map((c) => (c[0] as { id: string }).id),
    ).toEqual(pinLayers().map((l) => l.id));
  });

  it("opens the basemap without pins when there is no backend", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockRejectedValue(new Error("no backend"));
    render(MapPanel, { props: { open: true } });
    await waitFor(() => expect(mapInstance().addControl).toHaveBeenCalled());
    expect(mapInstance().addSource).not.toHaveBeenCalled();
  });

  it("draws cluster counts and thumbnail pins from the queried features", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue(PINS);
    render(MapPanel, { props: { open: true } });
    await waitFor(() => expect(MapMock).toHaveBeenCalled());
    const map = mapInstance();
    expect(map.on).toHaveBeenCalled();
    map.querySourceFeatures.mockReturnValue([
      {
        geometry: { coordinates: [13.4, 52.5] },
        properties: { cluster: true, point_count: 7 },
      },
      { geometry: { coordinates: [151.2, -33.9] }, properties: { hash: "h2" } },
    ] as never);
    fire(map, "moveend");
    await waitFor(() => expect(Marker).toHaveBeenCalledTimes(2));
    const elements = vi
      .mocked(Marker)
      .mock.calls.map((c) => (c[0] as { element: HTMLElement }).element);
    const cluster = elements.find((el) => el.dataset.testid === "map-cluster");
    expect(cluster?.textContent).toBe("7");
    const pin = elements.find((el) => el.dataset.testid === "map-pin");
    const img = pin?.querySelector("img");
    expect(img?.getAttribute("src")).toBe("thumb://h2");
  });

  it("refreshes markers after map data updates (zoom-in re-clusters)", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue(PINS);
    render(MapPanel, { props: { open: true } });
    await waitFor(() => expect(MapMock).toHaveBeenCalled());
    const map = mapInstance();
    expect(map.on).toHaveBeenCalled();
    map.querySourceFeatures.mockReturnValue([
      { geometry: { coordinates: [1, 2] }, properties: { hash: "h1" } },
    ] as never);
    fire(map, "moveend");
    fire(map, "sourcedata", { sourceId: "photos", isSourceLoaded: true });
    await waitFor(() =>
      expect(map.querySourceFeatures).toHaveBeenCalledWith("photos"),
    );
    expect(Marker).toHaveBeenCalled();
  });

  it("removes the markers when the panel closes", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue(PINS);
    const { rerender } = render(MapPanel, { props: { open: true } });
    await waitFor(() => expect(MapMock).toHaveBeenCalled());
    const map = mapInstance();
    map.querySourceFeatures.mockReturnValue([
      { geometry: { coordinates: [1, 2] }, properties: { hash: "h1" } },
    ] as never);
    fire(map, "moveend");
    await waitFor(() => expect(Marker).toHaveBeenCalled());
    const marker =
      vi.mocked(Marker).mock.results[vi.mocked(Marker).mock.results.length - 1]
        .value;
    rerender({ open: false });
    await waitFor(() => expect(marker.remove).toHaveBeenCalled());
  });
});
