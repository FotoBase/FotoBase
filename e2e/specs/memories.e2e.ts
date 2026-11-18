// TASK-07-008 e2e: the "On this day" Memories view. Photos captured on
// today's month/day in past years are grouped by year (newest first), a
// photo from another day stays out, and clicking a memory opens the normal
// viewer.

import fs from "node:fs";
import { $, $$, browser, expect } from "@wdio/globals";

import { dispatchClick } from "../support/interact";
import { makeMemoriesLibrary, type Library } from "../support/library";

let lib: Library & { years: [number, number] };

describe("memories view", () => {
  before(async () => {
    lib = makeMemoriesLibrary();
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
      async () => (await $$('[data-testid="grid-cell"]').length) === 4,
      { timeout: 20_000, timeoutMsg: "grid did not show the four photos" },
    );
  });

  after(() => {
    fs.rmSync(lib.dir, { recursive: true, force: true });
  });

  it("groups today's photos by year, newest year first", async () => {
    await dispatchClick('[data-testid="open-memories"]');
    await browser.waitUntil(
      async () => (await $$('[data-testid="memories-year"]').length) === 2,
      { timeout: 20_000, timeoutMsg: "memories did not show two year sections" },
    );
    const years = await $$('[data-testid="memories-year"]').map((el) =>
      el.getAttribute("data-year"),
    );
    expect(years).toEqual([String(lib.years[0]), String(lib.years[1])]);
    expect(await $$('[data-testid="memories-photo"]').length).toBe(3);
  });

  it("opening a memory photo shows the normal viewer", async () => {
    await dispatchClick('[data-testid="memories-photo"]');
    await $('[data-testid="viewer"]').waitForExist({
      timeout: 20_000,
      timeoutMsg: "viewer did not open from a memory",
    });
    expect(await $('[data-testid="viewer-count"]').getText()).toBe("1/3");
  });
});
