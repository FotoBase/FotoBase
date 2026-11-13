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

type maplibreGeoJSONSource = import("maplibre-gl").GeoJSONSource;

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("./map-src", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./map-src")>()),
  availableBasemap: vi.fn(),
}));
vi.mock("maplibre-gl", async (importOriginal) => ({
  ...(await importOriginal<typeof import("maplibre-gl")>()),
  Map: vi.fn().mockImplementation(function () {
    return {
      addControl: vi.fn(),
      remove: vi.fn(),
      addSource: vi.fn(),
      addLayer: vi.fn(),
      getSource: vi.fn().mockImplementation(
        (() => {
          const src = { setData: vi.fn() };
          return () => src;
        })(),
      ),
      querySourceFeatures: vi.fn(() => []),
      on: vi.fn(),
      once: vi.fn(),
      isStyleLoaded: vi.fn(() => true),
      // Drop-to-geotag maps the drop point to decimal degrees (TASK-06-005).
      unproject: vi.fn(() => ({ lat: 52.52, lng: 13.405 })),
    };
  }),
  Marker: vi.fn().mockImplementation(function (opts?: {
    element: HTMLElement;
  }) {
    return {
      setLngLat: vi.fn().mockReturnThis(),
      addTo: vi.fn().mockReturnThis(),
      remove: vi.fn(),
      getElement: () => opts?.element,
    };
  }),
  NavigationControl: vi.fn(),
}));

const PINS = [
  { id: 1, filename: "a.jpg", hash: "h1", latitude: 52.5, longitude: 13.4 },
  { id: 2, filename: "b.jpg", hash: "h2", latitude: -33.9, longitude: 151.2 },
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
  getSource: ReturnType<typeof vi.fn<(id: string) => unknown>>;
  querySourceFeatures: ReturnType<typeof vi.fn>;
  once: ReturnType<typeof vi.fn>;
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
      getSource: vi.fn().mockImplementation(
        (() => {
          const src = { setData: vi.fn() };
          return () => src;
        })(),
      ),
      querySourceFeatures: vi.fn(() => []),
      on: vi.fn(),
      once: vi.fn(),
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
    const mapInstance0 = mapInstance() as unknown as {
      addSource: { mock: { calls: [unknown, Record<string, unknown>][] } };
    };
    const [id, source] = mapInstance0.addSource.mock.calls[0];
    expect(id).toBe("photos");
    expect(source.cluster).toBe(true);
    expect(source.data).toEqual(pinsToGeoJson(PINS));
  });

  it("defers pin wiring until the style has loaded (addSource throws before)", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue(PINS);
    (
      MapMock as unknown as {
        mockImplementationOnce: (f: () => unknown) => void;
      }
    ).mockImplementationOnce(function () {
      return {
        addControl: vi.fn(),
        remove: vi.fn(),
        addSource: vi.fn(),
        addLayer: vi.fn(),
        getSource: vi.fn().mockImplementation(
          (() => {
            const src = { setData: vi.fn() };
            return () => src;
          })(),
        ),
        querySourceFeatures: vi.fn(() => []),
        on: vi.fn(),
        once: vi.fn(),
        isStyleLoaded: vi.fn(() => false),
      } as unknown as import("maplibre-gl").Map;
    });
    render(MapPanel, { props: { open: true } });
    await waitFor(() => expect(MapMock).toHaveBeenCalled());
    const map = mapInstance();
    expect(map.addSource).not.toHaveBeenCalled();
    await waitFor(() => {
      const load = map.once.mock.calls.find((c) => c[0] === "load");
      expect(load).toBeTruthy();
      (load![1] as () => void)();
    });
    expect(map.addSource).toHaveBeenCalledWith(
      "photos",
      expect.objectContaining({ cluster: true }),
    );
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
    await waitFor(() => expect(map.on).toHaveBeenCalled());
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
    await waitFor(() => expect(map.on).toHaveBeenCalled());
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
    await waitFor(() => expect(map.on).toHaveBeenCalled());
    map.querySourceFeatures.mockReturnValue([
      { geometry: { coordinates: [1, 2] }, properties: { hash: "h1" } },
    ] as never);
    fire(map, "moveend");
    await waitFor(() => expect(Marker).toHaveBeenCalled());
    vi.mocked(Marker).mock.calls.length;
    const marker =
      vi.mocked(Marker).mock.results[vi.mocked(Marker).mock.results.length - 1]
        .value;
    rerender({ open: false });
    await waitFor(() => expect(marker.remove).toHaveBeenCalled());
  });
});

// TASK-06-003: the map's pins mirror the grid's filtered view. The panel
// queries the filtered pin set and re-queries (debounced) when the active
// search/filter state changes, updating the clustered source in place.
describe("MapPanel — filtered pins", () => {
  beforeEach(() => {
    mockStatus(true);
  });

  const view = {
    folders: [1],
    album: null,
    query: "berlin",
    starred: true,
    videosOnly: false,
    hasFaces: false,
    geotagged: false,
    takenSince: "2024-01-01",
    sort: "path" as const,
  };

  it("queries the filtered pin set for the current view", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue([PINS[0]]);
    render(MapPanel, { props: { open: true, view } });
    await waitFor(() => expect(mapInstance().addSource).toHaveBeenCalled());
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("map_pins_filtered", {
      text: "berlin",
      folders: [1],
      album: null,
      starred: true,
      videosOnly: false,
      hasFaces: false,
      geotagged: false,
      takenSince: "2024-01-01",
    });
  });

  it("re-queries when the view changes and updates the source in place", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue([PINS[0]]);
    const { rerender } = render(MapPanel, { props: { open: true, view } });
    await waitFor(() => expect(mapInstance().addSource).toHaveBeenCalled());
    const map = mapInstance();
    const calls = vi.mocked(invoke).mock.calls.length;

    vi.mocked(invoke).mockResolvedValue([PINS[1]]);
    rerender({ open: true, view: { ...view, query: "sydney" } });
    await waitFor(() =>
      expect(vi.mocked(invoke).mock.calls.length).toBeGreaterThan(calls),
    );
    await waitFor(() => {
      expect(map.getSource).toHaveBeenCalledWith("photos");
      const source = map.getSource("photos") as unknown as { setData: (data: string) => void };
      expect(source.setData).toHaveBeenCalledWith(
        pinsToGeoJson([PINS[1]]),
      );
    });
  });

  it("debounces bursts of view changes into one re-query", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue(PINS);
    const { rerender } = render(MapPanel, { props: { open: true, view } });
    await waitFor(() => expect(mapInstance().addSource).toHaveBeenCalled());
    vi.mocked(invoke).mockClear();
    for (const q of ["a", "ab", "abc", "abcd"]) {
      rerender({ open: true, view: { ...view, query: q } });
    }
    await new Promise((r) => setTimeout(r, 450));
    expect(vi.mocked(invoke).mock.calls.length).toBe(1);
  });
});

// TASK-06-004: pin interactions. A plain click/tap opens the photo in the
// viewer; a modifier click (mouse) or a long-press (touch) toggles the
// pin's selection; the selected pins feed the Photo Tray through the
// panel's `ontray` callback. A clicked cluster zooms in instead.
describe("MapPanel — pin interactions", () => {
  beforeEach(() => {
    mockStatus(true);
  });

  /// Render with one pin on screen and return its marker element.
  async function renderWithPin(
    props: Record<string, unknown> = {},
  ): Promise<HTMLElement> {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue(PINS);
    render(MapPanel, { props: { open: true, ...props } });
    await waitFor(() => expect(MapMock).toHaveBeenCalled());
    const map = mapInstance();
    // Earlier tests leave a persistent mockReturnValue on querySourceFeatures
    // (mockClear keeps implementations); reset it before the initial draw so
    // the draw this test drives is the only one that has seen these features.
    map.querySourceFeatures.mockReset();
    map.querySourceFeatures.mockReturnValue([]);
    // The initial debounced pin load must have run before the features are
    // drawn — otherwise its async completion redraws mid-test and orphans
    // the element reference.
    await waitFor(() => expect(map.addSource).toHaveBeenCalled());
    await waitFor(() => expect(map.on).toHaveBeenCalled());
    map.querySourceFeatures.mockReturnValue([
      {
        geometry: { coordinates: [1, 2] },
        properties: { hash: "h1", id: 7, filename: "a.jpg" },
      },
    ] as never);
    fire(map, "moveend");
    await waitFor(() => expect(Marker).toHaveBeenCalled());
    return vi
      .mocked(Marker)
      .mock.calls.map((c) => (c[0] as { element: HTMLElement }).element)
      .find((el) => el.dataset.testid === "map-pin")!;
  }

  const clickEl = (el: HTMLElement, init: MouseEventInit = {}): void => {
    el.dispatchEvent(new MouseEvent("click", { bubbles: true, ...init }));
  };

  it("opens the photo in the viewer on a plain pin click", async () => {
    const onopenphoto = vi.fn();
    const el = await renderWithPin({ onopenphoto });
    clickEl(el);
    expect(onopenphoto).toHaveBeenCalledWith({
      id: 7,
      filename: "a.jpg",
      hash: "h1",
    });
  });

  it("does not open the viewer when a modifier click selects", async () => {
    const onopenphoto = vi.fn();
    const el = await renderWithPin({ onopenphoto });
    clickEl(el, { ctrlKey: true });
    expect(onopenphoto).not.toHaveBeenCalled();
  });

  it("toggles selection with a ctrl/meta/shift click and marks the pin", async () => {
    const el = await renderWithPin();
    clickEl(el, { metaKey: true });
    await waitFor(() => expect(el.className).toContain("selected"));
    // The tray action appears with the selected count.
    expect(screen.getByTestId("map-add-to-tray").textContent).toMatch(/1/);
    clickEl(el, { metaKey: true });
    await waitFor(() => expect(el.className).not.toContain("selected"));
    expect(screen.queryByTestId("map-add-to-tray")).toBeNull();
  });

  it("feeds the selected photos to the Photo Tray and clears the selection", async () => {
    const ontray = vi.fn();
    const el = await renderWithPin({ ontray });
    clickEl(el, { ctrlKey: true });
    await waitFor(() =>
      expect(screen.getByTestId("map-add-to-tray")).toBeTruthy(),
    );
    clickEl(screen.getByTestId("map-add-to-tray") as HTMLElement);
    expect(ontray).toHaveBeenCalledWith([
      { id: 7, filename: "a.jpg", hash: "h1" },
    ]);
    await waitFor(() =>
      expect(screen.queryByTestId("map-add-to-tray")).toBeNull(),
    );
  });

  it("toggles selection on a long-press (touch) without opening the viewer", async () => {
    const onopenphoto = vi.fn();
    const el = await renderWithPin({ onopenphoto });
    el.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true, pointerType: "touch" }),
    );
    await new Promise((r) => setTimeout(r, 650));
    el.dispatchEvent(
      new PointerEvent("pointerup", { bubbles: true, pointerType: "touch" }),
    );
    expect(el.className).toContain("selected");
    expect(onopenphoto).not.toHaveBeenCalled();
  });

  it("opens the viewer on a short touch tap", async () => {
    const onopenphoto = vi.fn();
    const el = await renderWithPin({ onopenphoto });
    el.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true, pointerType: "touch" }),
    );
    el.dispatchEvent(
      new PointerEvent("pointerup", { bubbles: true, pointerType: "touch" }),
    );
    clickEl(el);
    await new Promise((r) => setTimeout(r, 650));
    expect(onopenphoto).toHaveBeenCalledTimes(1);
  });
});

// TASK-06-005: drag-to-geotag. Dropping a Fotobase photo drag on the map
// geotags the dropped photos at the drop point; foreign drags fall through.
describe("MapPanel — drag-to-geotag", () => {
  beforeEach(() => {
    mockStatus(true);
    // An earlier describe's mockImplementation (Map lifecycle) persists past
    // clearAllMocks; restore the full implementation with unproject.
    vi.mocked(MapMock).mockImplementation(function () {
      return {
        addControl: vi.fn(),
        remove: vi.fn(),
        addSource: vi.fn(),
        addLayer: vi.fn(),
        getSource: vi.fn().mockImplementation(
          (() => {
            const src = { setData: vi.fn() };
            return () => src;
          })(),
        ),
        querySourceFeatures: vi.fn(() => []),
        on: vi.fn(),
        once: vi.fn(),
        isStyleLoaded: vi.fn(() => true),
        unproject: vi.fn(() => ({ lat: 52.52, lng: 13.405 })),
      } as unknown as import("maplibre-gl").Map;
    });
  });

  // jsdom has no DragEvent constructor; a plain Event with a patched
  // `dataTransfer` reaches the Svelte handlers the same way.
  function dragEvent(type: string, transfer: DataTransfer): Event {
    const e = new Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(e, "dataTransfer", { value: transfer });
    return e;
  }

  function fakeTransfer(withPhotos: boolean): DataTransfer {
    const data = new Map<string, string>();
    const transfer = {
      setData: (type: string, value: string) => void data.set(type, value),
      getData: (type: string) => data.get(type) ?? "",
      dropEffect: "none",
      effectAllowed: "uninitialized",
      get types() {
        return [...data.keys()];
      },
    } as unknown as DataTransfer;
    if (withPhotos)
      transfer.setData("application/x-fotobase-photos", JSON.stringify([7, 8]));
    return transfer;
  }

  async function renderMap(): Promise<HTMLElement> {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue([]);
    render(MapPanel, { props: { open: true } });
    await waitFor(() => expect(MapMock).toHaveBeenCalled());
    return screen.getByTestId("map-holder");
  }

  it("geotags the dropped photos at the drop point", async () => {
    const holder = await renderMap();
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear().mockResolvedValue(undefined);

    holder.dispatchEvent(
      dragEvent("dragover", fakeTransfer(true)),
    );
    holder.dispatchEvent(
      dragEvent("drop", fakeTransfer(true)),
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("geotag_photos", {
        fileIds: [7, 8],
        latitude: 52.52,
        longitude: 13.405,
      }),
    );
    await waitFor(() => expect(screen.getByTestId("map-geotag-note")).toBeTruthy());
  });

  it("ignores foreign drags", async () => {
    const holder = await renderMap();
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear();

    holder.dispatchEvent(
      dragEvent("dragover", fakeTransfer(false)),
    );
    holder.dispatchEvent(
      dragEvent("drop", fakeTransfer(false)),
    );
    expect(invoke).not.toHaveBeenCalled();
  });
});
