// TASK-06-001 e2e: the offline basemap, end to end against the real app.
// Opening the map panel renders the bundled PMTiles basemap through the
// `basemap://` scheme and pans/zooms — with the network stack instrumented
// so a single http(s) request fails the spec (the task's test-enforced
// no-network rule, PLAN §4 / ADR-0008).
//
// The instrumentation patches `fetch` and `XMLHttpRequest` on the page and
// records every URL they see. Tile loads happen through MapLibre's own
// fetcher in web workers, which a main-thread patch cannot observe — the
// structural guarantees cover those: the style (map-src.ts, unit-tested)
// contains no URL but the `basemap://` template, and the Rust-side
// `no_network_clients` gate forbids any network dependency in the tile
// path. Everything the frontend itself could fetch (glyphs, sprites,
// remote styles) IS main-thread-visible and asserted here.

import { $, browser, expect } from "@wdio/globals";

import { dispatchClick } from "../support/interact";
import type { Library } from "../support/library";
import { makeLibrary } from "../support/library";

let lib: Library;

describe("offline basemap", () => {
  before(async () => {
    lib = makeLibrary();
    await browser.waitUntil(async () =>
      browser.execute(() =>
        Boolean((window as unknown as { __TAURI__?: unknown }).__TAURI__),
      ),
    );
    // Instrument the network stack BEFORE the map ever opens.
    await browser.execute(() => {
      const w = window as unknown as {
        __fotobaseNetLog: string[];
        fetch: typeof fetch;
        XMLHttpRequest: typeof XMLHttpRequest;
      };
      w.__fotobaseNetLog = [];
      const log = (url: string): void => {
        w.__fotobaseNetLog.push(url);
      };
      const realFetch = w.fetch.bind(w);
      w.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
        log(String(typeof input === "string" ? input : (input as Request).url ?? input));
        return realFetch(input as RequestInfo, init);
      }) as typeof fetch;
      const RealXhrOpen = XMLHttpRequest.prototype.open;
      XMLHttpRequest.prototype.open = function (
        this: XMLHttpRequest,
        method: string,
        url: string | URL,
        ...rest: unknown[]
      ) {
        log(String(url));
        return (RealXhrOpen as (...a: unknown[]) => void).apply(this, [method, url, ...rest]);
      } as unknown as typeof XMLHttpRequest.prototype.open;
    });
  });

  it("opens the map panel with the offline basemap", async () => {
    await dispatchClick('[data-testid="open-map"]');
    // The panel is up and the availability check passed with a resolved
    // archive (the e2e app bundles the world basemap).
    await browser.waitUntil(
      async () => await $('[data-testid="map-holder"]').isExisting(),
      { timeoutMsg: "map panel did not open with a basemap" },
    );
    const empty = await $('[data-testid="map-empty"]').isExisting();
    expect(empty).toBe(false);
  });

  it("pans and zooms the basemap without a single network request", async () => {
    // Drive the map canvas through its public API (the canvas itself is
    // not clickable under xvfb): several pan/zoom rounds force tile loads
    // across zoom levels.
    await browser.execute(() => {
      const holder = document.querySelector('[data-testid="map-holder"]') as HTMLElement;
      holder.dispatchEvent(
        new CustomEvent("fotobase-e2e-nop") // touch the DOM once so the wait below re-checks
      );
    });
    // Wheel-zoom into the holder: MapLibre listens for wheel events.
    for (let i = 0; i < 3; i += 1) {
      await browser.execute(() => {
        const holder = document.querySelector('[data-testid="map-holder"]');
        if (!holder) return;
        holder.dispatchEvent(
          new WheelEvent("wheel", { deltaY: -240, clientX: 300, clientY: 200, bubbles: true }),
        );
      });
      await browser.pause(400);
    }
    // Pan with a drag sequence.
    await browser.execute(() => {
      const holder = document.querySelector('[data-testid="map-holder"]');
      if (!holder) return;
      const rect = holder.getBoundingClientRect();
      const opts = (dx: number, dy: number, type: string) =>
        new MouseEvent(type, {
          clientX: rect.left + rect.width / 2 + dx,
          clientY: rect.top + rect.height / 2 + dy,
          bubbles: true,
        });
      holder.dispatchEvent(opts(0, 0, "mousedown"));
      for (const [dx, dy] of [[40, 20], [80, 40], [120, 60]]) {
        holder.dispatchEvent(opts(dx, dy, "mousemove"));
      }
      holder.dispatchEvent(opts(120, 60, "mouseup"));
    });
    await browser.pause(600);

    const urls = await browser.execute(
      () => (window as unknown as { __fotobaseNetLog: string[] }).__fotobaseNetLog,
    );
    // Zero real-network requests while the map is open — the task's
    // test-enforced no-network rule. A failure lists what was fetched.
    // WebView2 maps custom schemes and Tauri IPC onto `http://*.localhost`
    // URLs; those never leave the machine, so they are not network.
    const network = urls.filter(
      (u) => /^https?:\/\//i.test(u) && !/^https?:\/\/[^/]+\.localhost(\/|$)/i.test(u),
    );
    if (network.length > 0) {
      throw new Error(`network requests observed while the map was open: ${network.join(", ")}`);
    }
    // Everything else the frontend fetched must be a basemap tile or
    // Tauri's own IPC / scheme traffic.
    const nonBasemap = urls.filter(
      (u) =>
        !u.startsWith("basemap://") &&
        !u.startsWith("ipc://") &&
        !/^https?:\/\/(basemap|thumb|preview|stream|ipc)\.localhost\//i.test(u),
    );
    if (nonBasemap.length > 0) {
      throw new Error(`non-basemap frontend requests: ${nonBasemap.join(", ")}`);
    }
  });

  it("closes the panel again", async () => {
    await dispatchClick('[data-testid="map-close"]');
    await browser.waitUntil(
      async () => !(await $('[data-testid="map-backdrop"]').isExisting()),
      { timeoutMsg: "map panel did not close" },
    );
  });
});
