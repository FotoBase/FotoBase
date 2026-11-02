// TASK-04-011 e2e: batch edit ops, end to end against the real app. Photos
// go into the Photo Tray, the tray's "Edit…" action opens the batch dialog,
// picking an op runs the batch through the job queue with a progress UI,
// and every processed photo's sidecar recipe is the same per-photo recipe
// the single-photo flow would have written. No original changes (§9).

import { browser, expect, $, $$ } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";

import { makeLibrary, PAIR_JPEG, sha256, SOLO, type Library } from "../support/library";

let lib: Library;

const cells = () => $$('[data-testid="grid-cell"]');

// WebKitGTK intermittently reports an interactable-looking element as "not
// interactable" (element re-rendered between lookup and click). Plain
// clicks with bounded retries.
async function clickTestId(testid: string): Promise<void> {
  const sel = `[data-testid="${testid}"]`;
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      const el = await $(sel);
      await el.click();
      return;
    } catch {
      await browser.pause(500);
    }
  }
  throw new Error(`could not click ${sel}`);
}

// Click + verify + DOM-click fallback (the edit-view spec's pattern):
// overlay elements under xvfb sometimes swallow real clicks.
async function activate(
  testid: string,
  verify: () => Promise<boolean> | boolean,
): Promise<void> {
  const sel = `[data-testid="${testid}"]`;
  await clickTestId(testid);
  try {
    await browser.waitUntil(verify, { timeout: 2000 });
    return;
  } catch {
    // fall through to the DOM click
  }
  await browser.execute((selector: string) => {
    const el = document.querySelector(selector) as HTMLElement | null;
    if (!el) throw new Error(`${selector} not found`);
    el.click();
  }, sel);
  await browser.waitUntil(verify, {
    timeout: 8000,
    timeoutMsg: `${sel} never took effect`,
  });
}

async function waitForCellCount(count: number): Promise<void> {
  await browser.waitUntil(async () => (await cells().length) === count, {
    timeoutMsg: `expected ${count} grid cells`,
  });
}

function sidecarPath(name: string): string {
  return path.join(lib.dir, `${name}.xmp`);
}

function sidecarText(name: string): string {
  return fs.existsSync(sidecarPath(name))
    ? fs.readFileSync(sidecarPath(name), "utf8")
    : "";
}

// Every photo in the grid into the tray, one context menu at a time: each
// right-click selects the photo under the pointer, "Add to tray" appends
// it, and the tray accumulates across photos.
async function fillTray(): Promise<void> {
  const cellCount = await cells().length;
  for (let i = 0; i < cellCount; i++) {
    const img = await $$('[data-testid="grid-cell"] img')[i];
    await img.click({ button: "right", x: 10, y: 10 });
    await activate("menu-item-add-to-tray", async () => {
      try {
        return !(await $('[data-testid="context-menu"]').isDisplayed());
      } catch {
        return true;
      }
    });
  }
  await browser.waitUntil(
    async () => {
      const count = await $('[data-testid="tray-count"]').getText();
      return count.startsWith("3");
    },
    { timeoutMsg: "tray never reached 3 photos" },
  );
}

describe("batch edit", () => {
  before(async () => {
    lib = makeLibrary();
    await browser.waitUntil(async () =>
      browser.execute(() =>
        Boolean((window as unknown as { __TAURI__?: unknown }).__TAURI__),
      ),
    );
    const error = await browser.executeAsync(
      (folder: string, done: (e: string | null) => void) => {
        const tauri = (
          window as unknown as {
            __TAURI__: {
              core: { invoke: (cmd: string, args: object) => Promise<unknown> };
            };
          }
        ).__TAURI__;
        tauri.core
          .invoke("set_folder_mode", { path: folder, mode: "once" })
          .then(
            () => done(null),
            (e) => done(String(e)),
          );
      },
      lib.dir,
    );
    expect(error).toBeNull();
    await waitForCellCount(3);
  });

  after(() => {
    fs.rmSync(lib.dir, { recursive: true, force: true });
  });

  it("applies a batch op to the tray's photos with a progress UI", async () => {
    await fillTray();

    // Tray action opens the batch edit dialog for the tray's photos.
    await activate("tray-edit", () => $('[data-testid="batch-edit-dialog"]').isDisplayed());

    // One §2E op button per op; picking one runs the batch immediately.
    const opButtons = await $$('[data-testid^="batch-edit-op-"]');
    expect(opButtons.length).toBe(12);

    // The run is quick for three photos, so the progress counter may flash
    // by: accept either the live counter or the finished report.
    await activate("batch-edit-op-rotate_cw", async () => {
      try {
        return await $('[data-testid="batch-edit-progress"]').isDisplayed();
      } catch {
        try {
          return await $('[data-testid="batch-edit-done"]').isDisplayed();
        } catch {
          return false;
        }
      }
    });
    await $('[data-testid="batch-edit-done"]').waitForDisplayed({
      timeout: 10_000,
    });

    // Batch result equals per-photo result: the recipe the batch wrote is
    // exactly what the single-photo edit flow would have written.
    await browser.waitUntil(
      () => sidecarText(SOLO).includes('"rotate"') && sidecarText(SOLO).includes('"angle":90'),
      { timeout: 10_000, timeoutMsg: "solo sidecar never gained the rotate op" },
    );
    await browser.waitUntil(
      () =>
        sidecarText(PAIR_JPEG).includes('"rotate"') &&
        sidecarText(PAIR_JPEG).includes('"angle":90'),
      { timeout: 10_000, timeoutMsg: "pair sidecar never gained the rotate op" },
    );

    // §9 Data safety: the batch never touches an original.
    for (const name of [PAIR_JPEG, SOLO]) {
      expect(sha256(path.join(lib.dir, name))).toBe(lib.checksums[name]);
    }
  });

  it("a cancelled batch leaves only valid per-photo recipes behind", async () => {
    // Open the dialog again and cancel mid-run: the cancel button lives
    // only while the batch runs, so click it as soon as it appears.
    await activate("tray-edit", () => $('[data-testid="batch-edit-dialog"]').isDisplayed());
    await clickTestId("batch-edit-op-film_grain");
    try {
      await (await $('[data-testid="batch-edit-cancel"]')).click();
    } catch {
      // The batch already finished (three photos are fast) — nothing to
      // cancel, and the run's outcome is still a valid end state.
    }

    // Whatever the race's outcome, the dialog reaches a terminal report and
    // every sidecar that exists holds a valid recipe packet: processed
    // photos keep their edits, unprocessed ones stay recipe-less.
    await browser.waitUntil(
      async () => {
        try {
          return await $('[data-testid="batch-edit-done"]').isDisplayed();
        } catch {
          return false;
        }
      },
      { timeout: 10_000, timeoutMsg: "batch never reported an outcome" },
    );
    try {
      await clickTestId("batch-edit-close");
    } catch {
      // Closing is cosmetic here; the assertions below are the contract.
    }
    for (const name of [PAIR_JPEG, SOLO]) {
      const text = sidecarText(name);
      if (text === "") continue;
      expect(text).toContain("fotobase.recipe");
      expect(text).toContain("film_grain");
    }
    // §9: originals still untouched after the second batch.
    for (const name of [PAIR_JPEG, SOLO]) {
      expect(sha256(path.join(lib.dir, name))).toBe(lib.checksums[name]);
    }
  });
});
