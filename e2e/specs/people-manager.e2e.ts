// TASK-05-006 e2e: the People Manager, end to end against the real app.
// With no face pass ever run on this fresh library there are no people and
// the manager reports it; opening and closing leaves the grid untouched.
//
// Note: seeding real people would require the AI models (opt-in download,
// TASK-05-001) plus face-bearing fixture photos, neither of which the e2e
// environment has — the named/unnamed listing and the rename flow are
// covered end to end by the Rust integration tests (catalog + app half)
// and the component tests.

import fs from "node:fs";

import { $$, browser, expect } from "@wdio/globals";

import { makeLibrary, type Library } from "../support/library";

let lib: Library;

const cells = () => $$('[data-testid="grid-cell"]');

describe("people manager", () => {
  before(async () => {
    lib = makeLibrary();
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

  it("opens empty and leaves the grid untouched", async () => {
    await browser.waitUntil(async () => (await cells().length) === 3);

    await $('[data-testid="open-people-manager"]').click();
    await $('[data-testid="pm-backdrop"]').waitForDisplayed();
    await expect($('[data-testid="pm-empty"]')).toBeDisplayed();

    await $('[data-testid="pm-close"]').click();
    await browser.waitUntil(async () => (await cells().length) === 3);
  });
});
