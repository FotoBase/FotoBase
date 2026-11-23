// TASK-08-018 e2e: the full-screen Timeline. Photos from three years group
// into three days, oldest first; the arrow keys scrub between them, and
// opening a day returns to the grid in capture order.

import fs from "node:fs";
import { $, $$, browser, expect } from "@wdio/globals";

import { dispatchClick } from "../support/interact";
import { makeTimelineLibrary, type Library } from "../support/library";

let lib: Library & { days: string[] };

const focusedDate = () =>
  browser.execute(
    () =>
      document.querySelector('[data-testid="timeline-scroll"]')?.getAttribute("data-focused-date") ??
      "",
  );

describe("timeline view", () => {
  before(async () => {
    lib = makeTimelineLibrary();
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

  it("opens on the newest day and scrubs back with the arrow keys", async () => {
    await dispatchClick('[data-testid="open-timeline"]');
    await browser.waitUntil(async () => (await focusedDate()) === lib.days[2], {
      timeout: 20_000,
      timeoutMsg: "timeline did not open on the newest day",
    });
    await browser.keys("ArrowLeft");
    expect(await focusedDate()).toBe(lib.days[1]);
    await browser.keys("ArrowLeft");
    expect(await focusedDate()).toBe(lib.days[0]);
    expect(await $('[data-testid="timeline-status"]').getText()).toContain("2 photos");
  });

  it("opening a day lands in the grid in capture order", async () => {
    await browser.keys("Enter");
    await $('[data-testid="timeline"]').waitForExist({ reverse: true, timeout: 20_000 });
    expect(await $('[data-testid="filter-sort"]').getValue()).toBe("capture");
    expect(await $$('[data-testid="grid-cell"]').length).toBe(4);
  });
});
