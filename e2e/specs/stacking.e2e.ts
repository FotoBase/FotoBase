// TASK-03-021 e2e: stacking, end to end against the real app. The grid shows
// one thumbnail with a badge for a stack, the badge opens the picker, the top
// can be switched, and unstacking brings every photo back — all without a
// byte of any original changing.

import fs from "node:fs";
import path from "node:path";
import { $, $$, browser, expect } from "@wdio/globals";

import { makeLibrary, PAIR_JPEG, PAIR_RAW, sha256, SOLO, type Library } from "../support/library";

const cells = () => $$('[data-testid="grid-cell"]');
const badges = () => $$('[data-testid="stack-badge"]');
const members = () => $$('[data-testid="stack-member"]');

// `textContent`, not getText(): WebDriver's visible-text rule drops text a
// stylesheet clips with overflow hidden + ellipsis (the filename columns).
const textOf = async (el: {
  getProperty(name: string): Promise<unknown>;
}): Promise<string> => String(await el.getProperty("textContent"));

let lib: Library;

const sidecar = (name: string): string => path.join(lib.dir, `${name}.xmp`);
const sidecarText = (name: string): string =>
  fs.existsSync(sidecar(name)) ? fs.readFileSync(sidecar(name), "utf8") : "";

async function waitForCellCount(count: number): Promise<void> {
  await browser.waitUntil(async () => (await cells().length) === count, {
    timeoutMsg: `expected ${count} grid cells`,
  });
}

describe("stacking", () => {
  before(async () => {
    lib = makeLibrary();
    await browser.waitUntil(async () =>
      browser.execute(() => Boolean((window as unknown as { __TAURI__?: unknown }).__TAURI__)),
    );
    // "once" scans the folder into the library now, like the Folder Manager.
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

  it("lists every photo of the library before anything is stacked", async () => {
    await waitForCellCount(3);
    expect(await badges().length).toBe(0);
  });

  it("offers the RAW+JPEG pair and stacks it into one thumbnail with a badge", async () => {
    await $('[data-testid="open-stack-suggestions"]').click();

    const suggestion = $('[data-testid="stack-suggestion"]');
    await suggestion.waitForDisplayed();
    expect(await $$('[data-testid="stack-suggestion"]').length).toBe(1);
    const row = await textOf(suggestion);
    expect(row).toContain(PAIR_JPEG);
    expect(row).toContain(PAIR_RAW);

    await $('[data-testid="stack-suggestion-accept"]').click();
    await $('[data-testid="stack-suggestions-empty"]').waitForDisplayed();
    await $('[data-testid="stack-suggestions-close"]').click();

    await waitForCellCount(2);
    expect(await badges().length).toBe(1);
    await expect(badges()[0]).toHaveText("2");
  });

  it("wrote the stack into both sidecars: one id, the JPEG on top", async () => {
    const jpeg = sidecarText(PAIR_JPEG);
    const raw = sidecarText(PAIR_RAW);
    expect(jpeg).toContain("<fb:StackRole>top</fb:StackRole>");
    expect(raw).toContain("<fb:StackRole>member</fb:StackRole>");
    const id = (text: string) => /<fb:StackId>([^<]+)<\/fb:StackId>/.exec(text)?.[1];
    expect(id(jpeg)).toBeTruthy();
    expect(id(jpeg)).toBe(id(raw));
    expect(fs.existsSync(sidecar(SOLO))).toBe(false);
  });

  it("opens the picker from the badge and switches the top of the stack", async () => {
    // TEMP debug
    console.log(
      "DEBUG badge:",
      await browser.execute(() => {
        const b = document.querySelector('[data-testid="stack-badge"]') as HTMLElement;
        const r = b.getBoundingClientRect();
        const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
        const cs = getComputedStyle(b);
        return JSON.stringify({
          rect: r,
          hit: hit?.outerHTML.slice(0, 200),
          display: cs.display,
          vis: cs.visibility,
          pe: cs.pointerEvents,
          win: [window.innerWidth, window.innerHeight, window.scrollY],
          overlays: document.querySelectorAll(".ss-backdrop,.stack-picker,.context-menu").length,
        });
      }),
    );
    await badges()[0].click();
    await $('[data-testid="stack-picker"]').waitForDisplayed();
    await browser.waitUntil(async () => (await members().length) === 2);
    expect(await textOf(members()[0])).toContain(PAIR_JPEG);
    await expect(members()[0]).toHaveAttribute("data-top", "true");

    await $('[data-testid^="stack-make-top-"]').click();

    await browser.waitUntil(async () => (await textOf(members()[0])).includes(PAIR_RAW), {
      timeoutMsg: "the RAW did not become the top",
    });
    expect(sidecarText(PAIR_RAW)).toContain("<fb:StackRole>top</fb:StackRole>");
    expect(sidecarText(PAIR_JPEG)).toContain("<fb:StackRole>member</fb:StackRole>");
    await waitForCellCount(2);
  });

  it("unstacks: both photos get their own thumbnail back and the sidecars let go", async () => {
    await $('[data-testid="stack-unstack"]').click();
    await $('[data-testid="stack-picker"]').waitForDisplayed({ reverse: true });

    await waitForCellCount(3);
    expect(await badges().length).toBe(0);
    expect(sidecarText(PAIR_JPEG)).not.toContain("fb:Stack");
    expect(sidecarText(PAIR_RAW)).not.toContain("fb:Stack");
  });

  it("never touched an original", () => {
    for (const [name, checksum] of Object.entries(lib.checksums)) {
      expect(sha256(path.join(lib.dir, name))).toBe(checksum);
    }
  });
});
