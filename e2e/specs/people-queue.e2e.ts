// TASK-05-005 e2e: the unnamed people queue, end to end against the real
// app. The queue is the People panel: with no face pass ever run on this
// fresh library the queue is empty and the panel reports it; opening and
// closing leaves the grid untouched.
//
// Note: seeding real clusters would require the AI models (opt-in download,
// TASK-05-001) plus face-bearing fixture photos, neither of which the e2e
// environment has — the bulk flow itself is covered end to end by the Rust
// integration tests (catalog + app half) and the component tests.

import fs from "node:fs";

import { $$, browser, expect } from "@wdio/globals";

import { makeLibrary, type Library } from "../support/library";

let lib: Library;

const cells = () => $$('[data-testid="grid-cell"]');

describe("unnamed people queue", () => {
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
  });

  after(() => {
    fs.rmSync(lib.dir, { recursive: true, force: true });
  });

  it("opens empty and leaves the grid untouched", async () => {
    await browser.waitUntil(async () => (await cells().length) === 3);

    await $('[data-testid="open-people"]').click();
    await $('[data-testid="people-backdrop"]').waitForDisplayed();
    await expect($('[data-testid="people-empty"]')).toBeDisplayed();

    await $('[data-testid="people-close"]').click();
    await browser.waitUntil(async () => (await cells().length) === 3);
  });
});
