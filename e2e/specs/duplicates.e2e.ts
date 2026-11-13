// TASK-04-019 e2e: the near-duplicate finder, end to end against the real
// app. Two byte-identical copies plus one distinct photo group into one
// reviewable set; confirming the delete moves the marked file to the OS
// trash and drops it from the grid; cancelling deletes nothing.

import fs from "node:fs";
import path from "node:path";
import { $, $$, browser, expect } from "@wdio/globals";

import { makeLibrary, PAIR_JPEG, SOLO, type Library } from "../support/library";

const cells = () => $$('[data-testid="grid-cell"]');

let lib: Library;

const DUPLICATE = "IMG_0001-copy.jpg";

// `textContent`, not getText(): WebDriver's visible-text rule drops text a
// stylesheet clips with overflow hidden + ellipsis (the member file names).
const textOf = async (el: {
  getProperty(name: string): Promise<unknown>;
}): Promise<string> => String(await el.getProperty("textContent"));

async function waitForCellCount(count: number): Promise<void> {
  await browser.waitUntil(async () => (await cells().length) === count, {
    timeoutMsg: `expected ${count} grid cells`,
  });
}

describe("near-duplicate finder", () => {
  before(async () => {
    lib = makeLibrary();
    // A byte-identical copy of the pair JPEG: a guaranteed duplicate.
    fs.copyFileSync(path.join(lib.dir, PAIR_JPEG), path.join(lib.dir, DUPLICATE));
    await browser.waitUntil(async () =>
      browser.execute(() => Boolean((window as unknown as { __TAURI__?: unknown }).__TAURI__)),
    );
    const error = (await browser.execute(async (folder: string) => {
        const tauri = (
          window as unknown as {
            __TAURI__: { core: { invoke: (cmd: string, args: object) => Promise<unknown> } };
          }
        ).__TAURI__;
        try {
          await tauri.core.invoke("set_folder_mode", { path: folder, mode: "once" });
          return null;
        } catch (e) {
          return String(e);
        }
      }, lib.dir)) as string | null;
    expect(error).toBeNull();
  });

  after(() => {
    fs.rmSync(lib.dir, { recursive: true, force: true });
  });

  it("lists every photo before anything is deleted", async () => {
    await waitForCellCount(4);
  });

  it("groups the duplicate pair for review", async () => {
    await $('[data-testid="open-duplicate-finder"]').click();
    await $('[data-testid="dupes-run"]').click();
    await $('[data-testid="dupe-group"]').waitForDisplayed();

    // All three JPEGs come from the same bytes in the fixture library, so
    // the group holds all of them. `textContent`, not getText(): WebDriver's
    // visible-text rule drops text a stylesheet clips with overflow hidden.
    const group = await textOf(await $('[data-testid="dupe-group"]'));
    expect(group).toContain(PAIR_JPEG);
    expect(group).toContain(DUPLICATE);
    expect(group).toContain(SOLO);
  });

  it("cancel deletes nothing", async () => {
    // wdio 10 strict selectors: the group holds three members, so the mark
    // button is not unique — mark the first row's member explicitly.
    await (await $$('[data-testid="dupe-mark"]'))[0].click();
    await $('[data-testid="dupes-delete"]').click();
    await $('[data-testid="dupes-cancel-delete"]').click();
    await $('[data-testid="dupes-close"]').click();
    await waitForCellCount(4);
    expect(fs.existsSync(path.join(lib.dir, PAIR_JPEG))).toBe(true);
    expect(fs.existsSync(path.join(lib.dir, DUPLICATE))).toBe(true);
  });

  it("confirm moves the marked file to the trash", async () => {
    await $('[data-testid="open-duplicate-finder"]').click();
    await $('[data-testid="dupes-run"]').waitForDisplayed();
    await $('[data-testid="dupe-group"]').waitForDisplayed();

    // Mark the copy by name: member order in the group is row-id order.
    const marks = await $$('[data-testid="dupe-mark"]');
    let marked = false;
    for (const mark of marks) {
      if ((await textOf(mark)).includes(DUPLICATE)) {
        await mark.click();
        marked = true;
        break;
      }
    }
    expect(marked).toBe(true);
    await $('[data-testid="dupes-delete"]').click();
    await $('[data-testid="dupes-confirm"]').click();
    await $('[data-testid="dupes-close"]').click();

    await waitForCellCount(3);
    expect(fs.existsSync(path.join(lib.dir, DUPLICATE))).toBe(false);
    // The keepers stay untouched on disk.
    expect(fs.existsSync(path.join(lib.dir, PAIR_JPEG))).toBe(true);
    expect(fs.existsSync(path.join(lib.dir, SOLO))).toBe(true);
  });
});
