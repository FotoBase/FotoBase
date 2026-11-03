// TASK-05-010 e2e: the per-folder face-detection toggle (PLAN §2 Tools menu
// "Folder Manager — face-detection toggle"), end to end against the real
// app. The Folder Manager dialog shows an enabled checkbox per folder; the
// state mirrors the catalog and toggling it round-trips through IPC.

import fs from "node:fs";
import { $, browser, expect } from "@wdio/globals";

import { makeLibrary, type Library } from "../support/library";

let lib: Library;

// WebDriver's async-script callback carries ONE result, so the reply is
// wrapped; a rejected command throws here.
type Reply = { ok: true; value: unknown } | { ok: false; error: string };
const invoke = async (cmd: string, args: object = {}): Promise<unknown> => {
  const reply = await browser.executeAsync<Reply, [string, object]>(
    (cmd: string, args: object, done: (r: Reply) => void) => {
      const tauri = (
        window as unknown as {
          __TAURI__: { core: { invoke: (c: string, a: object) => Promise<unknown> } };
        }
      ).__TAURI__;
      tauri.core.invoke(cmd, args).then(
        (value) => done({ ok: true, value }),
        (e) => done({ ok: false, error: String(e) }),
      );
    },
    cmd,
    args,
  );
  if (!reply.ok) throw new Error(reply.error);
  return reply.value;
};

describe("folder face-detection toggle", () => {
  before(async () => {
    lib = makeLibrary();
    await browser.waitUntil(async () =>
      browser.execute(() => Boolean((window as unknown as { __TAURI__?: unknown }).__TAURI__)),
    );
    await invoke("set_folder_mode", { path: lib.dir, mode: "once" });
  });

  after(() => {
    fs.rmSync(lib.dir, { recursive: true, force: true });
  });

  it("the dialog shows an enabled checkbox mirroring the catalog (off)", async () => {
    await $('button[data-testid="open-folder-manager"]').click();
    const toggle = await $(`input[data-testid="face-toggle-${lib.dir}"]`);
    await toggle.waitForExist({ timeout: 10_000 });
    expect(await toggle.isEnabled()).toBe(true);
    expect(await toggle.isSelected()).toBe(false);
  });

  it("toggling on persists and the catalog reflects it", async () => {
    await $(`input[data-testid="face-toggle-${lib.dir}"]`).click();
    const stored = await invoke("folder_face_enabled", { path: lib.dir });
    expect(stored).toBe(true);
    // The dialog keeps the checkbox in sync with the catalog state.
    expect(await $(`input[data-testid="face-toggle-${lib.dir}"]`).isSelected()).toBe(true);
  });

  it("toggling off persists too", async () => {
    await $(`input[data-testid="face-toggle-${lib.dir}"]`).click();
    const stored = await invoke("folder_face_enabled", { path: lib.dir });
    expect(stored).toBe(false);
  });
});
