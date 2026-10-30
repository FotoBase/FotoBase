// TASK-04-010 e2e: the edit view, end to end against the real app. Opening
// a photo shows the three edit tabs and the filmstrip; applying an op from
// each tab updates the preview; undo reverts it; and no byte of any
// original changes (§9 Data safety — Save is FEAT-04-06, not here).

import { browser, expect, $, $$ } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";

import { makeLibrary, sha256, SOLO, type Library } from "../support/library";

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
      ).__TAURI__;
      tauri.core.invoke(cmd, args).then(
        (value) => done({ ok: true, value }),
        (e) => done({ ok: false, error: String(e) }),
      );
    },
    cmd,
    args,
  );
  if (!reply.ok) throw new Error(`${cmd} failed: ${reply.error}`);
  return reply.value;
};

const cells = () => $$('[data-testid="grid-cell"]');

// WebKitGTK intermittently reports an interactable-looking element as "not
// interactable" (element re-rendered between lookup and click). Scroll into
// view and retry; the selector is named when giving up so failures say what
// could not be clicked.
async function clickTestId(testid: string): Promise<void> {
  const sel = `[data-testid="${testid}"]`;
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      const el = await $(sel);
      await el.scrollIntoView();
      await el.click();
      return;
    } catch {
      await browser.pause(500);
    }
  }
  throw new Error(`could not click ${sel}`);
}

async function waitForCellCount(count: number): Promise<void> {
  await browser.waitUntil(async () => (await cells().length) === count, {
    timeoutMsg: `expected ${count} grid cells`,
  });
}

// The debounced render saves the recipe ~250ms after the last edit.
async function settle(): Promise<void> {
  await browser.pause(700);
}

describe("edit view", () => {
  before(async () => {
    lib = makeLibrary();
    await browser.waitUntil(async () =>
      browser.execute(() => Boolean((window as unknown as { __TAURI__?: unknown }).__TAURI__)),
    );
    const error = await browser.executeAsync(
      (folder: string, done: (e: string | null) => void) => {
        const tauri = (
          window as unknown as {
            __TAURI__: { core: { invoke: (cmd: string, args: object) => Promise<unknown> } };
          }
        ).__TAURI__;
        tauri.core.invoke("set_folder_mode", { path: folder, mode: "once" }).then(
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

  it("opens the edit view with tabs, filmstrip and a live preview", async () => {
    // Open the viewer on the solo photo (third cell). A double-click is
    // unreliable in WebKitWebDriver (the first click's selection re-render
    // can swallow the dblclick), so use the keyboard path: click selects,
    // Enter opens.
    await cells()[2].click();
    await browser.keys(["Enter"]);
    await $('[data-testid="viewer"]').waitForDisplayed();
    await clickTestId("edit-open");
    await $('[data-testid="edit-view"]').waitForDisplayed();
    await $('[data-testid="edit-tab-basic"]').waitForDisplayed();
    await $('[data-testid="edit-tab-tuning"]').waitForDisplayed();
    await $('[data-testid="edit-tab-effects"]').waitForDisplayed();
    await $('[data-testid="edit-preview"]').waitForDisplayed();
    expect(await $$('[data-testid="edit-filmstrip-thumb"]').length).toBe(3);
  });

  it("applies an op from each tab and updates the preview", async () => {
    const before = await $('[data-testid="edit-preview"]').getAttribute("src");

    // Effects tab: B&W one-click.
    await clickTestId("edit-tab-effects");
    await $('[data-testid="effects"]').waitForDisplayed();
    await clickTestId("effect-bw");
    await settle();

    // Tuning tab: Fill Light slider.
    await clickTestId("edit-tab-tuning");
    await $('[data-testid="tuning"]').waitForDisplayed();
    await $('[data-testid="fill-light"]').scrollIntoView();
    await $('[data-testid="fill-light"]').setValue(0.5);
    await settle();

    // Basic Fixes tab: Auto Contrast.
    await clickTestId("edit-tab-basic");
    await $('[data-testid="auto-fix"]').waitForDisplayed();
    await clickTestId("auto-contrast");
    await settle();

    const after = await $('[data-testid="edit-preview"]').getAttribute("src");
    expect(after).toContain("?r=");
    expect(after).not.toBe(before);

    // The recipe persisted through the sidecar write-through.
    const sidecar = fs.readFileSync(path.join(lib.dir, `${SOLO}.xmp`), "utf8");
    expect(sidecar).toContain("fotobase.recipe");
    expect(sidecar).toContain('"bw"');
    expect(sidecar).toContain("fill_light");
    expect(sidecar).toContain("auto_contrast");
  });

  it("undo reverts the recipe", async () => {
    await clickTestId("edit-undo");
    await settle();
    const sidecar = fs.readFileSync(path.join(lib.dir, `${SOLO}.xmp`), "utf8");
    expect(sidecar).not.toContain("auto_contrast");
  });

  it("never modified the original (checksum-verified, §9)", async () => {
    expect(sha256(path.join(lib.dir, SOLO))).toBe(lib.checksums[SOLO]);
  });

  it("Escape closes the edit view", async () => {
    await browser.keys(["Escape"]);
    await $('[data-testid="edit-view"]').waitForDisplayed({ reverse: true });
    // The viewer underneath comes back (EditView overlays it, not replaces
    // the session).
    await $('[data-testid="viewer"]').waitForDisplayed();
  });
});
