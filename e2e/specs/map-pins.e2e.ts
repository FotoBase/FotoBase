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
      async () => await $('[data-testid="map-cluster"]').isExisting(),
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
    // Zoom in past the cluster max zoom. Wheel ticks only bump the zoom a
    // fraction of a level, so drive both wheel events and the keyboard
    // handler's full-level zoom ("+") until the pins resolve.
    for (let i = 0; i < 30; i += 1) {
      await browser.execute(() => {
        const holder = document.querySelector('[data-testid="map-holder"]');
        if (!holder) return;
        // Zoom anchored on the cluster itself (wheel zoom keeps the cursor
        // point fixed — zooming at a fixed offset would drift off it).
        const cluster = document.querySelector('[data-testid="map-cluster"]');
        const rect = (cluster ?? holder).getBoundingClientRect();
        holder.dispatchEvent(
          new WheelEvent("wheel", {
            deltaY: -240,
            clientX: rect.left + rect.width / 2,
            clientY: rect.top + rect.height / 2,
            bubbles: true,
          }),
        );
        holder.dispatchEvent(
          new KeyboardEvent("keydown", { key: "+", bubbles: true }),
        );
      });
      await browser.pause(150);
      if (await $('[data-testid="map-pin"] img').isExisting()) break;
    }
    await browser.waitUntil(
      async () => await $('[data-testid="map-pin"] img').isExisting(),
      {
        timeout: 15_000,
        timeoutMsg: "zoom-in did not resolve clusters into photo pins",
      },
    );
    const src = await $('[data-testid="map-pin"] img').getAttribute("src");
    // Thumbnails travel through the custom scheme, never IPC (§3 rule 3).
    expect(src?.startsWith("thumb://")).toBe(true);
  });

  it("closes the panel again", async () => {
    await dispatchClick('[data-testid="map-close"]');
    await browser.waitUntil(
      async () => !(await $('[data-testid="map-backdrop"]').isExisting()),
      { timeoutMsg: "map panel did not close" },
    );
  });
});
