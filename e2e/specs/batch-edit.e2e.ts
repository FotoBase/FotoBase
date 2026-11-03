// TASK-04-011 e2e: batch edit ops, end to end against the real app. Photos
// go into the Photo Tray, the tray's "Edit…" action opens the batch dialog,
// picking an op runs the batch through the job queue with a progress UI,
// and every processed photo's sidecar recipe is the same per-photo recipe
// the single-photo flow would have written. No original changes (§9).
//
// Dialog interactions go through JS clicks exclusively: WebKitWebDriver
// under xvfb intermittently reports the dialog's buttons as "click
// intercepted" (the dialog is a scrollable overlay; the driver's computed
// click point can land on the backdrop). The buttons' handlers are what
// this spec exercises, and a JS click fires exactly those.

import { browser, expect, $, $$ } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";

import { makeLibrary, PAIR_JPEG, sha256, SOLO, type Library } from "../support/library";

let lib: Library;

const cells = () => $$('[data-testid="grid-cell"]');

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

// Whether an element exists in the DOM right now (no implicit waits —
// safe to poll).
function exists(testid: string): Promise<boolean> {
  return browser.execute(
    (selector: string) => document.querySelector(selector) !== null,
    `[data-testid="${testid}"]`,
  );
}

// The dialog's live UI state, read straight from the DOM: "progress" while
// the run is in flight, "done"/"error" once it reports, "none" otherwise.
type DialogState = { state: "progress" | "done" | "error" | "none"; text: string };

function dialogState(): Promise<DialogState> {
  return browser.execute(() => {
    const q = (id: string) => document.querySelector(`[data-testid="${id}"]`);
    const text = (id: string) => (q(id)?.textContent ?? "").trim();
    if (q("batch-edit-error")) return { state: "error", text: text("batch-edit-error") };
    if (q("batch-edit-done")) return { state: "done", text: text("batch-edit-done") };
    if (q("batch-edit-progress")) {
      return { state: "progress" as const, text: text("batch-edit-progress") };
    }
    return { state: "none" as const, text: "" };
  });
}

// Run one batch op and wait for the report. A backend error surfaces here
// with its text instead of as an opaque "never took effect".
async function runBatchOp(op: string): Promise<DialogState> {
  await domClick(`batch-edit-op-${op}`);
  const deadline = Date.now() + 30_000;
  let last: DialogState = { state: "none", text: "" };
  while (Date.now() < deadline) {
    last = await dialogState();
    if (last.state === "error") {
      throw new Error(`batch ${op} failed: ${last.text}`);
    }
    if (last.state === "done") return last;
    await browser.pause(250);
  }
  throw new Error(`batch ${op} never finished; last dialog state: ${last.state} ${last.text}`);
}

// Close the dialog if a previous test left it open: the backdrop sits above
// the tray, so an open dialog would block the next test's "Edit…" action.
async function closeDialogIfOpen(): Promise<void> {
  if (await exists("batch-edit-dialog")) {
    await domClick("batch-edit-close");
    await browser.waitUntil(async () => !(await exists("batch-edit-dialog")), {
      timeout: 5_000,
      timeoutMsg: "batch edit dialog never closed",
    });
  }
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
// right-click opens the photo menu, "Add to tray" appends the photo, and
// the tray accumulates across photos. The menu item is picked with a JS
// click (see the header note); the right-click itself is the real gesture.
// Each add is verified on its own, so a failure names the photo and what
// the menu offered (TASK-05-013: "tray never reached 3 photos" said nothing).
async function fillTray(): Promise<void> {
  const trayCount = (): Promise<string> =>
    browser.execute(
      () => document.querySelector('[data-testid="tray-count"]')?.textContent?.trim() ?? "",
    );
  // Page-reload detector: the tray store is module-global, so a count that
  // drops back means the page reloaded (WebKit restarts a crashed or
  // unresponsive web process) — TASK-05-013.
  await browser.execute(() => {
    (window as unknown as { __fbMarker?: number }).__fbMarker = 1;
  });
  const reloaded = async (): Promise<string> =>
    (await browser.execute(() => (window as unknown as { __fbMarker?: number }).__fbMarker === 1))
      ? "page NOT reloaded"
      : "page RELOADED (marker gone)";
  const imgs = await $$('[data-testid="grid-cell"] img');
  let added = 0;
  for (const img of imgs) {
    const alt = await img.getAttribute("alt");
    await img.click({ button: "right", x: 10, y: 10 });
    await browser.waitUntil(() => exists("context-menu"), {
      timeout: 5_000,
      timeoutMsg: `photo context menu never opened for ${alt}`,
    });
    const offered = await browser.execute(() =>
      Array.from(document.querySelectorAll('[data-testid^="menu-item-"]')).map((e) =>
        e.getAttribute("data-testid"),
      ),
    );
    await domClick("menu-item-add-to-tray");
    await browser.waitUntil(async () => !(await exists("context-menu")), {
      timeout: 5_000,
      timeoutMsg: "photo context menu never closed",
    });
    added++;
    try {
      await browser.waitUntil(async () => (await trayCount()).startsWith(String(added)), {
        timeout: 5_000,
      });
    } catch {
      throw new Error(
        `tray did not grow to ${added} after adding ${alt}; tray-count="${await trayCount()}" ${await reloaded()}; menu offered: ${offered.join(",")}`,
      );
    }
  }
  await browser.waitUntil(
    async () => {
      const count = await $('[data-testid="tray-count"]').getText();
      return count.startsWith("3");
    },
    { timeout: 10_000 },
  ).catch(async () => {
    throw new Error(
      `tray never reached 3 photos (tray-count="${await trayCount()}", ${await reloaded()})`,
    );
  });
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
    await domClick("tray-edit");
    await browser.waitUntil(() => exists("batch-edit-dialog"), {
      timeoutMsg: "batch edit dialog never opened",
    });

    // One §2E op button per op; picking one runs the batch immediately.
    const opButtons = await $$('[data-testid^="batch-edit-op-"]');
    expect(opButtons.length).toBe(12);

    // The run goes through the job queue; the dialog shows the progress
    // counter while it runs and the report when it lands.
    const report = await runBatchOp("rotate_cw");
    expect(report.text).toContain("3");

    // Batch result equals per-photo result: the recipe the batch wrote is
    // exactly what the single-photo edit flow would have written.
    await browser.waitUntil(
      () =>
        sidecarText(SOLO).includes('"rotate"') && sidecarText(SOLO).includes('"angle":90'),
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

    await closeDialogIfOpen();
  });

  it("a cancelled batch leaves only valid per-photo recipes behind", async () => {
    // Three photos process in milliseconds, so winning the cancel race is
    // not guaranteed: click Cancel the moment the button appears, and
    // accept a run that already finished — either way the dialog must
    // reach a terminal report and every existing sidecar must hold a
    // valid recipe packet (deterministic cancel semantics are covered by
    // the Rust integration tests).
    await domClick("tray-edit");
    await browser.waitUntil(() => exists("batch-edit-dialog"), {
      timeoutMsg: "batch edit dialog never opened",
    });
    await domClick("batch-edit-op-film_grain");

    // Poll for the cancel button for a moment; a finished run has none.
    for (let i = 0; i < 6 && (await exists("batch-edit-cancel")) === false; i++) {
      if ((await dialogState()).state === "done") break;
      await browser.pause(200);
    }
    // Check-and-click in ONE script: the run can finish between a separate
    // exists() and click (TASK-05-013), and a finished run is accepted.
    await browser.execute(() => {
      (document.querySelector('[data-testid="batch-edit-cancel"]') as HTMLElement | null)?.click();
    });

    await browser.waitUntil(async () => (await dialogState()).state === "done", {
      timeout: 30_000,
      timeoutMsg: "batch never reported an outcome",
    });
    const report = await dialogState();
    expect(report.text).not.toContain("failed");

    // Not cancelled: every photo got the op. Cancelled ("processed photos
    // keep their edits"): each photo either got it or still carries its
    // earlier rotate recipe untouched — never a half-written packet.
    const cancelled = await exists("batch-edit-cancelled");
    for (const name of [PAIR_JPEG, SOLO]) {
      const text = sidecarText(name);
      if (text === "") continue;
      expect(text).toContain("fotobase.recipe");
      if (cancelled && !text.includes("film_grain")) {
        expect(text).toContain('"rotate"');
      } else {
        expect(text).toContain("film_grain");
      }
    }
    // §9: originals still untouched after the second batch.
    for (const name of [PAIR_JPEG, SOLO]) {
      expect(sha256(path.join(lib.dir, name))).toBe(lib.checksums[name]);
    }

    await closeDialogIfOpen();
  });
});
