// TASK-06-001 component tests: the map panel. The availability check gates
// everything — no archive, no canvas, just the empty state. With an
// archive the map is constructed against the `basemap://` style (the
// no-network invariant itself is pinned in map-src.test.ts) and removed on
// close. maplibre-gl is mocked: jsdom has no WebGL and the real
// constructor would only test the mock anyway.

import { cleanup, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import MapPanel from "./MapPanel.svelte";
import { availableBasemap, mapStyle } from "./map-src";
import { Map as MapMock, NavigationControl } from "maplibre-gl";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("./map-src", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./map-src")>()),
  availableBasemap: vi.fn(),
}));
vi.mock("maplibre-gl", () => ({
  Map: vi.fn().mockImplementation(function () {
    return { addControl: vi.fn(), remove: vi.fn() };
  }),
  NavigationControl: vi.fn(),
}));

function mockStatus(available: boolean): void {
  vi.mocked(availableBasemap).mockResolvedValue(
    available ? { available, source: "/bundle" } : { available, source: null },
  );
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
    const instance = { addControl: vi.fn(), remove: vi.fn() };
    vi.mocked(MapMock).mockImplementation(function () {
      return instance;
    });
    const { rerender } = render(MapPanel, { props: { open: true } });
    await waitFor(() => expect(instance.addControl).toHaveBeenCalled());
    rerender({ open: false });
    await waitFor(() => expect(instance.remove).toHaveBeenCalled());
    expect(NavigationControl).toHaveBeenCalled();
  });
});
