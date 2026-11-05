// TASK-06-004 e2e: pin interactions. A plain pin click opens the photo in
// the viewer; selecting pins (modifier click on a mouse profile, long-press
// on a touch profile) feeds the Photo Tray, and the tray holds exactly the
// selected photos. Synthetic wheel/key events don't reach MapLibre under
// WebKitWebDriver, so the map is driven through its public API and the pins
// through DOM events, as in map-pins.e2e.ts.

import { $, $$, browser, expect } from "@wdio/globals";

import { dispatchClick } from "../support/interact";
import { makeGeotaggedLibrary, type Library } from "../support/library";

const COUNT = 40;

let lib: Library;

/// filename → the hash a pin's `thumb://` src carries (the catalog stores
/// the first 32 hex chars of the SHA-256).
let fileNameByPinHash: Map<string, string>;

/// DOM-dispatch a click with modifiers on the nth pin marker (document
/// order). A selector pseudo-class would miscount: pins and clusters
/// interleave as siblings.
async function clickPin(
  n: number,
  mods: { ctrlKey?: boolean; metaKey?: boolean; shiftKey?: boolean } = {},
): Promise<void> {
  const why = await browser.execute(
    (n: number, mods: object) => {
      const el = document.querySelectorAll('[data-testid="map-pin"]')[n] as
        | HTMLElement
        | undefined;
      if (!el) return `pin ${n} not found`;
      el.dispatchEvent(new MouseEvent("click", { bubbles: true, ...mods }));
      return null;
    },
    n,
    mods,
  );
  if (why !== null) throw new Error(why);
}

/// Long-press (pointer events, touch type) on the nth pin: the touch
/// selection path, where no modifier key exists.
async function longPressPin(n: number): Promise<void> {
  const down = await browser.execute((n: number) => {
    const el = document.querySelectorAll('[data-testid="map-pin"]')[n] as
      | HTMLElement
      | undefined;
    if (!el) return `pin ${n} not found`;
    el.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, pointerType: "touch" }));
    return null;
  }, n);
  if (down !== null) throw new Error(down);
  await browser.pause(650);
  const up = await browser.execute((n: number) => {
    const el = document.querySelectorAll('[data-testid="map-pin"]')[n] as
      | HTMLElement
      | undefined;
    el?.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, pointerType: "touch" }));
    return null;
  }, n);
  if (up !== null) throw new Error(up);
}

/// The thumb hash of the nth pin.
async function pinHashOf(n: number): Promise<string> {
  const src = await browser.execute((n: number) => {
    const el = document.querySelectorAll('[data-testid="map-pin"]')[n] as
      | HTMLElement
      | undefined;
    return el?.querySelector("img")?.getAttribute("src") ?? null;
  }, n);
  return (src ?? "").replace("thumb://", "");
}

describe("map pin selection", () => {
  before(async () => {
    lib = makeGeotaggedLibrary(COUNT);
    fileNameByPinHash = new Map(
      Object.entries(lib.checksums).map(([name, hash]) => [hash.slice(0, 32), name]),
    );
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
      { timeout: 20_000, timeoutMsg: "scan did not produce all geotagged pins" },
    );
    // Zoom in so every photo is its own pin, as in map-pins.e2e.ts.
    await dispatchClick('[data-testid="open-map"]');
    await browser.waitUntil(
      async () =>
        await browser.execute(() => {
          const holder = document.querySelector(
            '[data-testid="map-holder"]',
          ) as (HTMLElement & { __fbMap?: unknown }) | null;
          return Boolean(holder?.__fbMap);
        }),
      { timeoutMsg: "map handle not exposed" },
    );
    await browser.execute(() => {
      const holder = document.querySelector('[data-testid="map-holder"]') as
        | (HTMLElement & { __fbMap?: { jumpTo(o: unknown): void } })
        | null;
      holder?.__fbMap?.jumpTo({ center: [10.07, 25.07], zoom: 12 });
    });
    await browser.pause(1_000);
    await browser.waitUntil(
      async () => (await $$('[data-testid="map-pin"]').length) >= COUNT,
      { timeout: 15_000, timeoutMsg: "zoom-in did not resolve all pins" },
    );
  });

  it("opens the photo in the viewer on a pin click", async () => {
    // The clicked pin's thumbnail hash names the photo the viewer must show.
    const hash = await pinHashOf(0);
    await clickPin(0);
    await browser.waitUntil(
      async () => await $('[data-testid="viewer"]').isExisting(),
      { timeoutMsg: "pin click did not open the viewer" },
    );
    const shown = await $('[data-testid="viewer-main"]').getAttribute("alt");
    expect(shown).toBe(fileNameByPinHash.get(hash));
    // Close the viewer (Escape — the viewer's own key handler).
    await browser.keys(["Escape"]);
    await browser.waitUntil(
      async () => !(await $('[data-testid="viewer"]').isExisting()),
      { timeoutMsg: "viewer did not close" },
    );
  });

  it("feeds selected pins into the Photo Tray", async () => {
    // Modifier-click selects without opening the viewer; a long-press
    // selects a second pin (the touch path).
    await clickPin(0, { ctrlKey: true });
    await browser.waitUntil(
      async () => await $('[data-testid="map-add-to-tray"]').isExisting(),
      { timeoutMsg: "selection did not appear" },
    );
    await longPressPin(1);
    await browser.waitUntil(
      async () => (await $('[data-testid="map-add-to-tray"]').getText()).includes("2"),
      { timeoutMsg: "second pin did not join the selection" },
    );
    await dispatchClick('[data-testid="map-add-to-tray"]');
    await browser.waitUntil(
      async () => (await $$('[data-testid="tray-item"]').length) === 2,
      { timeout: 10_000, timeoutMsg: "tray did not receive the selected pins" },
    );
    // The tray holds exactly the selected photos — their filenames come
    // from the pin → photo mapping, not the grid selection.
    const names = (await browser.execute(() =>
      Array.from(
        document.querySelectorAll<HTMLImageElement>('[data-testid="tray-jump"] img'),
        (img) => img.getAttribute("alt"),
      ),
    )) as (string | null)[];
    expect(names.sort()).toEqual(
      [
        fileNameByPinHash.get(await pinHashOf(0)),
        fileNameByPinHash.get(await pinHashOf(1)),
      ].sort(),
    );
    // Clean up so later specs start with an empty tray.
    await dispatchClick('[data-testid="map-close"]');
    await dispatchClick('[data-testid="tray-clear"]');
    await browser.waitUntil(async () => await $('[data-testid="tray-empty"]').isExisting(), {
      timeoutMsg: "tray did not clear",
    });
  });

  it("selects on a touch long-press without opening the viewer", async () => {
    await dispatchClick('[data-testid="open-map"]');
    await browser.waitUntil(
      async () => await $('[data-testid="map-pin"]').isExisting(),
      { timeoutMsg: "map pins missing" },
    );
    await longPressPin(2);
    await browser.waitUntil(
      async () => await $('[data-testid="map-add-to-tray"]').isExisting(),
      { timeoutMsg: "long-press did not select the pin" },
    );
    expect(await $('[data-testid="viewer"]').isExisting()).toBe(false);
    await dispatchClick('[data-testid="map-close"]');
  });
});
