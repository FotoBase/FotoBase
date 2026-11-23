// TASK-08-013 e2e: Tools → Upload sends the Photo Tray to the local-folder
// plugin with the selected size. Originals remain byte-for-byte unchanged.

import { $, browser, expect } from "@wdio/globals";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { makeGeotaggedLibrary, sha256, type Library } from "../support/library";

let lib: Library;
let destinationRoot: string;
let destination: string;

type Reply = { ok: true; value: unknown } | { ok: false; error: string };
const invoke = async (cmd: string, args: object = {}): Promise<unknown> => {
  const reply = (await browser.execute(
    async (cmd: string, args: object) => {
      const tauri = (
        window as unknown as {
          __TAURI__: { core: { invoke: (c: string, a: object) => Promise<unknown> } };
        }
      ).__TAURI__;
      try {
        return { ok: true as const, value: await tauri.core.invoke(cmd, args) };
      } catch (e) {
        return { ok: false as const, error: String(e) };
      }
    },
    cmd,
    args,
  )) as Reply;
  if (!reply.ok) throw new Error(`${cmd} failed: ${reply.error}`);
  return reply.value;
};

async function exists(testid: string): Promise<boolean> {
  return browser.execute(
    (id: string) => document.querySelector(`[data-testid="${id}"]`) !== null,
    testid,
  );
}

async function domClick(testid: string): Promise<void> {
  await browser.execute((id: string) => {
    const element = document.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
    if (!element) throw new Error(`${id} not found`);
    if (element instanceof HTMLButtonElement && element.disabled) {
      throw new Error(`${id} is disabled`);
    }
    element.click();
  }, testid);
}

describe("Tools upload", () => {
  before(async () => {
    lib = makeGeotaggedLibrary(3);
    destinationRoot = fs.mkdtempSync(path.join(os.tmpdir(), "fb-e2e-upload-"));
    destination = path.join(destinationRoot, "output");
    fs.mkdirSync(destination);

    await browser.waitUntil(async () =>
      browser.execute(() => Boolean((window as unknown as { __TAURI__?: unknown }).__TAURI__)),
    );
    await invoke("set_folder_mode", { path: lib.dir, mode: "once" });
    for (let index = 0; index < 3; index += 1) {
      const filename = `geo_${String(index).padStart(3, "0")}.jpg`;
      await browser.waitUntil(
        async () =>
          browser.execute(
            (name: string) =>
              document.querySelector(`[data-testid="grid-cell"] img[alt="${name}"]`) !== null,
            filename,
          ),
        { timeoutMsg: `grid never showed ${filename}` },
      );
    }
  });

  after(() => {
    fs.rmSync(lib.dir, { recursive: true, force: true });
    fs.rmSync(destinationRoot, { recursive: true, force: true });
  });

  it("uploads three tray photos unchanged through Tools → Upload", async () => {
    for (let index = 0; index < 3; index += 1) {
      const filename = `geo_${String(index).padStart(3, "0")}.jpg`;
      const image = await $(`[data-testid="grid-cell"] img[alt="${filename}"]`);
      await image.click({ button: "right", x: 10, y: 10 });
      await browser.waitUntil(() => exists("context-menu"), {
        timeoutMsg: `context menu never opened for ${filename}`,
      });
      await domClick("menu-item-add-to-tray");
      await browser.waitUntil(() => exists("context-menu").then((found) => !found), {
        timeoutMsg: "context menu never closed",
      });
    }

    await browser.waitUntil(async () => {
      const count = await $('[data-testid="tray-count"]').getText();
      return count.startsWith("3");
    }, { timeoutMsg: "Photo Tray never reached three photos" });

    await domClick("open-upload");
    await browser.waitUntil(() => exists("upload-backdrop"), {
      timeoutMsg: "Upload dialog never opened",
    });
    await $('[data-testid="upload-target"]').selectByAttribute("value", "local-folder");
    await $('[data-testid="upload-local-folder"]').setValue(destination);
    await domClick("upload-size-original");
    await domClick("upload-run");

    await browser.waitUntil(
      () =>
        browser.execute(() => {
          const files = Array.from(
            document.querySelectorAll('[data-testid^="upload-file-"]'),
          );
          return files.length === 3 && files.every((file) =>
            file.textContent?.includes("Uploaded"),
          );
        }),
      { timeout: 30_000, timeoutMsg: "Upload did not report all three files as uploaded" },
    );

    const names = fs.readdirSync(destination).sort();
    expect(names).toEqual(["geo_000.jpg", "geo_001.jpg", "geo_002.jpg"]);
    for (const name of names) {
      expect(fs.readFileSync(path.join(destination, name))).toEqual(
        fs.readFileSync(path.join(lib.dir, name)),
      );
      expect(sha256(path.join(lib.dir, name))).toBe(lib.checksums[name]);
    }
  });
});
