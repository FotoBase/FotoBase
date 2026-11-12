// TASK-07-003 e2e: the live search bar degrades gracefully to FTS-only
// while the CLIP model is not downloaded (PLAN §1 opt-in, §6 P3). Typing
// must still narrow the grid live, the semantic indicator must stay off,
// and the "enable semantic search" hint must appear — no model, no
// download, no blocked grid.

import { $, $$, browser, expect } from "@wdio/globals";

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

describe("semantic search fallback (no model)", () => {
  it("typing narrows the grid without the model, and the hint shows", async () => {
  // No opt-in model in the e2e environment: the semantic path must be
  // refused, never block the grid, and point at Options.
  const status = (await browser.execute(async () => {
    const status = await (
      window as unknown as {
        __TAURI__: {
          core: { invoke: (cmd: string) => Promise<unknown> };
        };
      }
    ).__TAURI__.core.invoke("clip_model_status");
    return status as { installed: boolean };
  })) as { installed: boolean };
  expect(status.installed).toBe(false);

  const input = await $('[data-testid="search-input"]');
  await input.setValue("solo");
  await browser.waitUntil(
    async () => (await $$('[data-testid="grid-cell"]').length) === 1,
    { timeout: 20_000, timeoutMsg: "FTS-only search did not narrow the grid" },
  );
  // The semantic blend never contributed (no model), so the indicator
  // stays off while the enable hint appears.
  await browser.waitUntil(async () => Boolean(await $('[data-testid="semantic-hint"]')), {
    timeout: 20_000,
    timeoutMsg: "the semantic-search enable hint did not appear",
  });
  // `$$` (not `$`): WebDriver's `$` resolves missing elements to a truthy
  // error-carrying stub, which would defeat the falsy check.
  expect((await $$('[data-testid="semantic-indicator"]')).length).toBe(0);
});
});
