// TASK-09-003 e2e: mobile viewport layout and touch-only browse/view gestures.

import { $, browser, expect } from "@wdio/globals";
import fs from "node:fs";

import { makeLibrary, type Library } from "../support/library";

let lib: Library;

async function setFolder(path: string): Promise<void> {
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
  }, path)) as string | null;
  expect(error).toBeNull();
}

async function dispatchStageTouch(
  type: string,
  pointerId: number,
  clientX: number,
  clientY: number,
): Promise<void> {
  await browser.execute((type: string, pointerId: number, x: number, y: number) => {
    const stage = document.querySelector('[data-testid="viewer-stage"]') as HTMLElement | null;
    if (!stage) throw new Error("viewer stage not found");
    Object.defineProperty(stage, "setPointerCapture", {
      configurable: true,
      value: () => {},
    });
    stage.dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        cancelable: true,
        pointerId,
        pointerType: "touch",
        clientX: x,
        clientY: y,
      }),
    );
  }, type, pointerId, clientX, clientY);
}

describe("mobile touch browsing", () => {
  before(async () => {
    lib = makeLibrary();
    await browser.waitUntil(() =>
      browser.execute(() =>
        Boolean((window as unknown as { __TAURI__?: unknown }).__TAURI__),
      ),
    );
    await setFolder(lib.dir);
    await browser.waitUntil(async () => (await $$('[data-testid="grid-cell"]')).length === 3, {
      timeoutMsg: "mobile fixture photos did not load",
    });
  });

  after(() => {
    fs.rmSync(lib.dir, { recursive: true, force: true });
  });

  it("lays out portrait and landscape grids, opens by tap, swipes and pinches", async () => {
    await browser.setWindowSize(844, 900);
    await browser.waitUntil(async () =>
      (await $(".grid-row").getAttribute("style"))?.includes("repeat(5,"),
    );
    await browser.setWindowSize(844, 390);
    await browser.waitUntil(async () => (await $('[data-testid="grid-scroll"]').getSize()).height >= 160);

    await browser.setWindowSize(390, 844);
    await browser.waitUntil(async () =>
      (await $(".grid-row").getAttribute("style"))?.includes("repeat(2,"),
    );

    await browser.execute(() => {
      const nativeMatchMedia = window.matchMedia.bind(window);
      Object.defineProperty(window, "__fbNativeMatchMedia", {
        configurable: true,
        value: nativeMatchMedia,
      });
      Object.defineProperty(window, "matchMedia", {
        configurable: true,
        value: (query: string) =>
          query === "(pointer: coarse)"
            ? ({ matches: true } as MediaQueryList)
            : nativeMatchMedia(query),
      });
      const cell = document.querySelector('[data-testid="grid-cell"]');
      if (!cell) throw new Error("grid cell not found");
      cell.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    await expect($('[data-testid="viewer-stage"]')).toBeDisplayed();
    await expect($('[data-testid="star-toggle"]')).not.toBeDisplayed();
    await expect($('[data-testid="edit-open"]')).not.toBeDisplayed();

    await dispatchStageTouch("pointerdown", 1, 260, 100);
    await dispatchStageTouch("pointerup", 1, 90, 104);
    await browser.waitUntil(async () =>
      (await $('[data-testid="viewer-count"]').getText()).startsWith("2/"),
    );

    await dispatchStageTouch("pointerdown", 2, 100, 100);
    await dispatchStageTouch("pointerdown", 3, 200, 100);
    await dispatchStageTouch("pointermove", 3, 260, 100);
    await browser.waitUntil(async () =>
      Number(await $('[data-testid="viewer-stage"]').getAttribute("data-zoom")) > 1,
    );
    await $('[data-testid="viewer-close"]').click();
    await browser.waitUntil(async () => !(await $('[data-testid="viewer-stage"]').isExisting()));
    await browser.execute(() => {
      const current = window as Window & {
        __fbNativeMatchMedia?: typeof window.matchMedia;
      };
      if (current.__fbNativeMatchMedia) {
        Object.defineProperty(window, "matchMedia", {
          configurable: true,
          value: current.__fbNativeMatchMedia,
        });
        delete current.__fbNativeMatchMedia;
      }
    });
  });
});
