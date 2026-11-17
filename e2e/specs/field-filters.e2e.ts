// TASK-07-005 e2e: typing a field filter in the search bar narrows the
// grid live (PLAN §2 2026 additions "Advanced search syntax"). The e2e
// library has no EXIF Make/Model and no named faces, so the date fields
// (`before:` / `after:`, resolved against the mtime fallback of the
// capture-date rule) are the ones a fixture can exercise end to end;
// `person:` / `camera:` with valid syntax but no matches must empty the
// grid, never error.

import { $, $$, browser, expect } from "@wdio/globals";

import { dispatchClick } from "../support/interact";
import { makeLibrary } from "../support/library";

let libDir: string;

before(async () => {
  libDir = makeLibrary().dir;
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
  }, libDir);
  await browser.waitUntil(
    async () => (await $$('[data-testid="grid-cell"]').length) === 3,
    { timeout: 20_000, timeoutMsg: "grid did not show three photos" },
  );
});

describe("field filter syntax", () => {
  it("a date field filter narrows the grid live", async () => {
    const input = await $('[data-testid="search-input"]');
    // Every photo was stamped 2024-06-01: `before:2024` matches nothing.
    await input.setValue("before:2024");
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === 0,
      { timeout: 20_000, timeoutMsg: "before:2024 did not empty the grid" },
    );
    // `after:2024` (inclusive lower bound) keeps all three.
    await input.setValue("after:2024");
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === 3,
      { timeout: 20_000, timeoutMsg: "after:2024 did not keep all three" },
    );
    // A bare year and an ISO date bracket the same set.
    await input.setValue("after:2024-05-31 before:2024-06-02");
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === 3,
      { timeout: 20_000, timeoutMsg: "ISO date bounds did not keep all three" },
    );
  });

  it("valid filters with no matches empty the grid without erroring", async () => {
    const input = await $('[data-testid="search-input"]');
    await input.setValue("person:anna");
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === 0,
      { timeout: 20_000, timeoutMsg: "person:anna did not empty the grid" },
    );
    await input.setValue("camera:zzz");
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === 0,
      { timeout: 20_000, timeoutMsg: "camera:zzz did not empty the grid" },
    );
  });

  it("clearing the search restores the grid", async () => {
    const input = await $('[data-testid="search-input"]');
    await input.setValue("before:2024");
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === 0,
      { timeout: 20_000, timeoutMsg: "before:2024 did not empty the grid" },
    );
    // `setValue("")` is not an option: WebDriver rejects an empty text
    // parameter. The search bar's clear button is the user path.
    await dispatchClick('[data-testid="search-clear"]');
    await browser.waitUntil(
      async () => (await $$('[data-testid="grid-cell"]').length) === 3,
      { timeout: 20_000, timeoutMsg: "clearing did not restore the grid" },
    );
  });
});
