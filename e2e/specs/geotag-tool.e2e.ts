// TASK-06-006 e2e: Tools → Geotag, end to end against the real app. The
// photo context menu opens the geotag dialog; one pass applies the location
// to the selection with a progress UI, every sidecar carries the GPS, the
// original's bytes never change, and Undo restores the prior values.

import fs from "node:fs";
import path from "node:path";
import { browser, expect } from "@wdio/globals";

import { withDeadline } from "../support/interact";
import { makeLibrary, SOLO, type Library } from "../support/library";

let lib: Library;

type Reply = { ok: true; value: unknown } | { ok: false; error: string };
const invoke = async (cmd: string, args: object = {}): Promise<unknown> => {
  const reply = await browser.executeAsync<Reply, [string, object]>(
    (cmd: string, args: object, done: (r: Reply) => void) => {
      const tauri = (
        window as unknown as {
          __TAURI__: {
            core: { invoke: (c: string, a: object) => Promise<unknown> };
          };
        }
      ).__TAURI__.core.invoke(cmd, args).then(
        (value) => done({ ok: true, value }),
        (e) => done({ ok: false, error: String(e) }),
      );
    },
    cmd,
    args,
  );
  if (!reply.ok) throw new Error(reply.error);
  return reply.value;
};

// The context menu opens on the cell's `contextmenu` event; under xvfb +
// WebKitWebDriver a native right-click hangs, so the whole flow — find the
// solo cell, dispatch the event, click the menu item — runs as ONE page
// task (the same pattern reset-faces uses).
const openGeotagDialog = async (): Promise<void> => {
  const why = await withDeadline(
    browser.executeAsync(
      (alt: string, done: (r: "ok" | "no-cell" | "no-menu") => void): void => {
        let tries = 0;
        const attempt = (): void => {
          const img = document.querySelector(`img[alt="${alt}"]`);
          const cell =
            img?.closest('[data-testid="grid-cell"]') ??
            document.querySelector('[data-testid="grid-cell"]');
          if (!cell) {
            if (++tries > 100) return done("no-cell");
            setTimeout(attempt, 100);
            return;
          }
          cell.dispatchEvent(
            new MouseEvent("contextmenu", {
              bubbles: true,
              cancelable: true,
              clientX: 40,
              clientY: 40,
            }),
          );
          let waits = 0;
          const wait = (): void => {
            const item = document.querySelector(
              '[data-testid="menu-item-geotag"]',
            ) as HTMLElement | null;
            if (item) {
              item.click();
              return done("ok");
            }
            if (++waits > 50) return done("no-menu");
            setTimeout(wait, 100);
            return;
          };
          wait();
        };
        attempt();
      },
      SOLO,
    ),
    30_000,
    "open geotag dialog",
  );
  if (why !== "ok") throw new Error(`open geotag dialog: ${why}`);
};

// JS click on the element's own handler: no coordinates, no interception.
async function domClick(testid: string): Promise<void> {
  const sel = `[data-testid="${testid}"]`;
  await browser.execute((selector: string) => {
    const el = document.querySelector(selector) as HTMLElement | null;
    if (!el) throw new Error(`${selector} not found`);
    if (el instanceof HTMLButtonElement && el.disabled) {
      throw new Error(`${selector} is disabled`);
    }
    el.click();
  }, sel);
}

async function domType(testid: string, value: string): Promise<void> {
  const sel = `[data-testid="${testid}"]`;
  await browser.execute(
    (selector: string, v: string) => {
      const el = document.querySelector(selector) as HTMLInputElement | null;
      if (!el) throw new Error(`${selector} not found`);
      el.value = v;
      el.dispatchEvent(new Event("input", { bubbles: true }));
    },
    sel,
    value,
  );
}

interface Gps {
  lat: number | null;
  lon: number | null;
}

const sidecarGps = async (): Promise<Gps> => {
  const raw = (await invoke("photo_info", { fileId: await soloId() })) as {
    latitude: number | null;
    longitude: number | null;
  };
  return { lat: raw.latitude, lon: raw.longitude };
}

interface PhotoInfo {
  id: number;
  filename: string;
  latitude: number | null;
  longitude: number | null;
}

let soloIdCache: number | null = null;
const soloId = async (): Promise<number> => {
  if (soloIdCache !== null) return soloIdCache;
  const rows = (await invoke("query_files", { limit: 100 })) as PhotoInfo[];
  const row = rows.find((r) => r.filename === SOLO);
  if (!row) throw new Error("solo photo not in catalog");
  soloIdCache = row.id;
  return row.id;
};

describe("geotag tool", () => {
  before(async () => {
    lib = makeLibrary();
    await browser.waitUntil(async () =>
      browser.execute(() =>
        Boolean((window as unknown as { __TAURI__?: unknown }).__TAURI__),
      ),
    );
    await invoke("set_folder_mode", { path: lib.dir, mode: "once" });
    await browser.waitUntil(async () => (await soloId()) !== null, {
      timeout: 20_000,
      timeoutMsg: "scan did not produce the solo photo",
    });
  });

  after(() => {
    fs.rmSync(lib.dir, { recursive: true, force: true });
  });

  it("geotags the selection with progress and undoes it", async () => {
    const before = fs.readFileSync(path.join(lib.dir, SOLO));
    expect(await sidecarGps()).toEqual({ lat: null, lon: null });

    await openGeotagDialog();
    await browser.waitUntil(async () => await $('[data-testid="geotag-dialog"]').isExisting(), {
      timeoutMsg: "geotag dialog did not open",
    });
    await domType("geotag-lat", "52.5200");
    await domType("geotag-lon", "13.4050");
    await domClick("geotag-apply");

    // Progress or the done note: both prove the pass ran through the queue.
    await browser.waitUntil(
      async () =>
        (await $('[data-testid="geotag-progress"]').isExisting()) ||
        (await $('[data-testid="geotag-done"]').isExisting()),
      { timeoutMsg: "no geotag progress" },
    );
    await browser.waitUntil(async () => await $('[data-testid="geotag-done"]').isExisting(), {
      timeoutMsg: "geotag pass did not complete",
    });
    const gps = await sidecarGps();
    expect(gps.lat).toBeCloseTo(52.52, 5);
    expect(gps.lon).toBeCloseTo(13.405, 5);
    expect(fs.readFileSync(path.join(lib.dir, SOLO))).toEqual(before);

    // Undo restores the untagged state.
    await domClick("geotag-undo");
    await browser.waitUntil(async () => await $('[data-testid="geotag-undone"]').isExisting(), {
      timeoutMsg: "geotag undo did not complete",
    });
    expect(await sidecarGps()).toEqual({ lat: null, lon: null });
  });
});
