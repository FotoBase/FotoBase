// TASK-06-002 e2e: clustered map pins against the real app. A geotagged
// library (sidecar GPS — the source of truth) appears as counted cluster
// markers at the world zoom; zooming in resolves clusters into individual
// photo pins whose thumbnails come through the `thumb://` scheme. The
// frame-budget side of the perf spec runs through the perf-acceptance
// harness on the 50k fixture (PLAN §9); here we pin the behavior and that
// the interactions complete in bounded time.

import { $, $$, browser, expect } from "@wdio/globals";

import { dispatchClick } from "../support/interact";
import { makeGeotaggedLibrary, type Library } from "../support/library";

const COUNT = 40;

let lib: Library;

describe("map pin clustering", () => {
  before(async () => {
    lib = makeGeotaggedLibrary(COUNT);
    await browser.waitUntil(async () =>
      browser.execute(() =>
        Boolean((window as unknown as { __TAURI__?: unknown }).__TAURI__),
      ),
    );
    await browser.execute(async (dir: string) => {
      await (
        window as unknown as {
          __TAURI__: {
            core: { invoke: (cmd: string, args: unknown) => Promise<unknown> };
          };
        }
      ).__TAURI__.core.invoke("set_folder_mode", { path: dir, mode: "once" });
    }, lib.dir);
    // The scan must have synced the sidecar GPS before the map opens.
    await browser.waitUntil(
      async () => {
        const n = await browser.execute(async () => {
          const pins = await (
            window as unknown as {
              __TAURI__: {
                core: { invoke: (cmd: string) => Promise<unknown[]> };
              };
            }
          ).__TAURI__.core.invoke("map_pins");
          return (pins as unknown[]).length;
        });
        return n === COUNT;
      },
      {
        timeout: 20_000,
        timeoutMsg: "scan did not produce all geotagged pins",
      },
    );
  });

  it("shows counted clusters at the world zoom", async () => {
    await dispatchClick('[data-testid="open-map"]');
    await browser.waitUntil(
      async () => (await $$('[data-testid="map-cluster"]')).length > 0,
      { timeoutMsg: "no cluster markers at the world zoom" },
    );
    // All photos sit in one tight grid, so the world view must show a
    // single cluster carrying the whole library — the e2e form of the
    // "counts match photo totals" gate.
    const clusters = await $$('[data-testid="map-cluster"]');
    expect(clusters.length).toBe(1);
    expect(Number(await clusters[0].getText())).toBe(COUNT);
  });

  it("resolves clusters into thumbnail pins on zoom-in", async () => {
    // Synthetic wheel/key events don't reach MapLibre under
    // WebKitWebDriver, so drive the map through its public API (the panel
    // exposes the instance on the holder for the e2e specs). Zoom well
    // past the cluster max zoom, centered on the cluster.
    await browser.waitUntil(
      async () =>
        await browser.execute(() => {
          const holder = document.querySelector(
            '[data-testid="map-holder"]',
          ) as
            | (HTMLElement & {
                __fbMap?: {
                  getCenter(): [number, number];
                  jumpTo(o: unknown): void;
                };
              })
            | null;
          return Boolean(holder?.__fbMap);
        }),
      { timeoutMsg: "map handle not exposed" },
    );
    await browser.execute(() => {
      const holder = document.querySelector('[data-testid="map-holder"]') as
        | (HTMLElement & {
            __fbMap?: {
              getCenter(): [number, number];
              jumpTo(o: unknown): void;
            };
          })
        | null;
      const map = holder?.__fbMap;
      if (!map) return;
      // The fixture's grid is centered slightly NE of the initial view.
      map.jumpTo({ center: [10.06, 25.07], zoom: 12 });
    });
    await browser.pause(1_000);
    await browser.waitUntil(
      async () => (await $$('[data-testid="map-pin"] img')).length > 0,
      {
        timeout: 15_000,
        timeoutMsg: "zoom-in did not resolve clusters into photo pins",
      },
    );
    const src = await (await $$('[data-testid="map-pin"] img'))[0].getAttribute("src");
    // Thumbnails travel through the custom scheme, never IPC (§3 rule 3).
    expect(src?.startsWith("thumb://")).toBe(true);
  });

  it("applies the current search to the pins and updates live", async () => {
    // TASK-06-003: the map's pins mirror the grid's filtered view. Close
    // the panel, narrow the library with the search bar, reopen.
    await dispatchClick('[data-testid="map-close"]');
    await browser.waitUntil(
      async () => !(await $('[data-testid="map-backdrop"]').isExisting()),
      { timeoutMsg: "map panel did not close" },
    );
    const input = await $('[data-testid="search-input"]');
    await input.setValue("001");
    // geo_001.jpg is the only fixture file matching the "001" token.
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === 1,
      {
        timeout: 20_000,
        timeoutMsg: "search did not narrow the grid to one photo",
      },
    );
    await dispatchClick('[data-testid="open-map"]');
    await browser.waitUntil(
      async () => (await $$('[data-testid="map-pin"]')).length > 0,
      { timeoutMsg: "filtered map shows no pin" },
    );
    const pins = await $$('[data-testid="map-pin"]');
    expect(pins.length).toBe(1);
    // The pin's thumbnail is keyed by the content hash (§3 rule 3).
    const src = await pins[0].$("img").getAttribute("src");
    // The pin's thumbnail is keyed by the catalog's content hash — the
    // scanner stores the first 16 bytes of the SHA-256 as 32 hex chars
    // (§3 rule 3: the URI is built from the queried feature alone).
    expect(src).toBe(`thumb://${lib.checksums["geo_001.jpg"].slice(0, 32)}`);

    // Live update: widening the search re-pins the map (debounced).
    await dispatchClick('[data-testid="map-close"]');
    await browser.waitUntil(
      async () => !(await $('[data-testid="map-backdrop"]').isExisting()),
      { timeoutMsg: "map panel did not close" },
    );
    await input.setValue("00");
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === 10,
      {
        timeout: 20_000,
        timeoutMsg: "widened search did not match ten photos",
      },
    );
    await dispatchClick('[data-testid="open-map"]');
    // Ten photos sit in two adjacent grid columns → one cluster at the
    // world zoom, carrying exactly the filtered count.
    await browser.waitUntil(
      async () => {
        const clusters = await $$('[data-testid="map-cluster"]');
        return (
          (await clusters.length) === 1 &&
          (await clusters[0].getText()) === "10"
        );
      },
      { timeout: 20_000, timeoutMsg: "map pins did not follow the search" },
    );
  });

  it("closes the panel again", async () => {
    await dispatchClick('[data-testid="map-close"]');
    await browser.waitUntil(
      async () => !(await $('[data-testid="map-backdrop"]').isExisting()),
      { timeoutMsg: "map panel did not close" },
    );
  });
});
