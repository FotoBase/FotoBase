// TASK-06-007 e2e: the "Geotagged" filter button over a mixed library
// (PLAN §2C filter buttons, §6 P3 Places). Toggling it narrows the grid to
// exactly the photos whose sidecar GPS the catalog mirrored; combined with
// a search term it ANDs like every other filter; in map view the filter
// state is implicit and the pins mirror the filtered grid (TASK-06-003).

import { $, $$, browser, expect } from "@wdio/globals";

import { dispatchClick } from "../support/interact";
import { makeGeotaggedLibrary } from "../support/library";

const GEOTAGGED = 10;
const PLAIN = 5;

let libDir: string;

describe("geotagged filter", () => {
  before(async () => {
    libDir = makeGeotaggedLibrary(GEOTAGGED, PLAIN).dir;
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
    }, libDir);
    // The scan must have synced the sidecar GPS before the filter is toggled.
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
        return n === GEOTAGGED;
      },
      {
        timeout: 20_000,
        timeoutMsg: "scan did not produce all geotagged pins",
      },
    );
  });

  it("without the filter the grid shows the whole mixed library", async () => {
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === GEOTAGGED + PLAIN,
      { timeout: 20_000, timeoutMsg: "grid did not show all 15 photos" },
    );
    const pressed = await $('[data-testid="filter-geotagged"]').getAttribute("aria-pressed");
    expect(pressed).toBe("false");
  });

  it("toggling the filter narrows the grid to the geotagged photos", async () => {
    await dispatchClick('[data-testid="filter-geotagged"]');
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === GEOTAGGED,
      { timeout: 20_000, timeoutMsg: "filter did not narrow the grid to 10 photos" },
    );
    const pressed = await $('[data-testid="filter-geotagged"]').getAttribute("aria-pressed");
    expect(pressed).toBe("true");

    // Off again restores the whole library.
    await dispatchClick('[data-testid="filter-geotagged"]');
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === GEOTAGGED + PLAIN,
      { timeout: 20_000, timeoutMsg: "un-toggling did not restore the grid" },
    );
  });

  it("combines with a search term (ANDs, like every filter)", async () => {
    // "geo" matches the ten geotagged photos' filenames; "plain" the five
    // untagged ones. Either way the filter must drop the untagged half.
    await dispatchClick('[data-testid="filter-geotagged"]');
    const input = await $('[data-testid="search-input"]');
    await input.setValue("plain");
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === 0,
      { timeout: 20_000, timeoutMsg: "search+filter did not empty the grid" },
    );
    await input.setValue("geo");
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === GEOTAGGED,
      { timeout: 20_000, timeoutMsg: "search+filter did not keep the geotagged ten" },
    );
  });

  it("the map mirrors the filtered grid (the filter state is implicit there)", async () => {
    // Still filtered to the geotagged ten with the "geo" search active:
    // the map shows those ten pins in one cluster.
    await dispatchClick('[data-testid="open-map"]');
    await browser.waitUntil(
      async () => {
        const clusters = await $$('[data-testid="map-cluster"]');
        return (
          (await clusters.length) === 1 &&
          (await clusters[0].getText()) === String(GEOTAGGED)
        );
      },
      { timeout: 20_000, timeoutMsg: "map did not show the filtered ten as one cluster" },
    );
    await dispatchClick('[data-testid="map-close"]');
    await browser.waitUntil(
      async () => !(await $('[data-testid="map-backdrop"]').isExisting()),
      { timeoutMsg: "map panel did not close" },
    );
  });

  it("cleans up: filter off, search cleared", async () => {
    await dispatchClick('[data-testid="filter-geotagged"]');
    const input = await $('[data-testid="search-input"]');
    await input.setValue("");
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === GEOTAGGED + PLAIN,
      { timeout: 20_000, timeoutMsg: "library did not widen back" },
    );
  });
});
