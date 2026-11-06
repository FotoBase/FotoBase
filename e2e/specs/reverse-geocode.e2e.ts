// TASK-06-008 e2e: offline reverse geocoding from the embedded GeoNames
// dataset (PLAN §6 P3 Places). Geotagged photos resolve to city-level
// place names in the catalog; the FTS `place` column makes "Lisbon" a
// search term; the bulk pass runs as a background job, so the grid stays
// responsive while it converges.

import { $$, browser, expect } from "@wdio/globals";

import { dispatchClick } from "../support/interact";
import { makeGeotaggedLibrary } from "../support/library";

const GEOTAGGED = 10;
const PLAIN = 5;

let libDir: string;

describe("reverse geocode", () => {
  before(async () => {
    libDir = makeGeotaggedLibrary(GEOTAGGED, PLAIN, {
      lat: 38.7167,
      lon: -9.1394,
    }).dir;
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
    // The bulk pass runs at background priority after the scan's GPS
    // sync; wait for it to converge on all geotagged photos.
    await browser.waitUntil(
      async () => {
        const n = await browser.execute(async () => {
          const hits = await (
            window as unknown as {
              __TAURI__: {
                core: {
                  invoke: (cmd: string, args: unknown) => Promise<unknown[]>;
                };
              };
            }
          ).__TAURI__.core.invoke("search_files", {
            query: "Lisbon",
            limit: 100,
          });
          return (hits as unknown[]).length;
        });
        return n === GEOTAGGED;
      },
      {
        timeout: 30_000,
        timeoutMsg: "reverse geocode did not tag all photos Lisbon",
      },
    );
  });

  it("place names land in FTS: search 'Lisbon' finds the geotagged photos", async () => {
    const n = await browser.execute(async () => {
      const hits = await (
        window as unknown as {
          __TAURI__: {
            core: {
              invoke: (cmd: string, args: unknown) => Promise<unknown[]>;
            };
          };
        }
      ).__TAURI__.core.invoke("search_files", { query: "Lisbon", limit: 100 });
      return (hits as unknown[]).length;
    });
    expect(n).toBe(GEOTAGGED);
  });

  it("unresolvable coordinates (open ocean cluster) stay placeless", async () => {
    const n = await browser.execute(async () => {
      const hits = await (
        window as unknown as {
          __TAURI__: {
            core: {
              invoke: (cmd: string, args: unknown) => Promise<unknown[]>;
            };
          };
        }
      ).__TAURI__.core.invoke("search_files", { query: "Tokyo", limit: 100 });
      return (hits as unknown[]).length;
    });
    expect(n).toBe(0);
  });

  it("search by place ANDs into the grid like every other term", async () => {
    const input = await $('[data-testid="search-input"]');
    await input.setValue("Lisbon");
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === GEOTAGGED,
      {
        timeout: 20_000,
        timeoutMsg: "grid did not narrow to the Lisbon photos",
      },
    );
  });

  it("cleans up: search cleared", async () => {
    await dispatchClick('[data-testid="search-clear"]');
    await browser.waitUntil(
      async () =>
        (await $$('[data-testid="grid-cell"]').length) === GEOTAGGED + PLAIN,
      { timeout: 20_000, timeoutMsg: "library did not widen back" },
    );
  });
});
