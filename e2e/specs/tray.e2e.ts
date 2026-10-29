// TASK-03-025 e2e: tray/background watching, end to end against the real
// app. Closing the main window must keep the process (and the watcher)
// alive; a file dropped into a watched folder while the window is hidden
// still lands in the catalog; and quitting (the exact code path the tray
// menu's Quit item runs — WebDriver cannot click the OS tray, so the spec
// exposes the same `tray::quit` as the `quit_from_tray` command) fully
// exits with no orphan process.

import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { browser, expect } from "@wdio/globals";

import { makeLibrary, type Library } from "../support/library";

let lib: Library;

const invoke = (cmd: string, args: object = {}): Promise<unknown> =>
  browser.executeAsync(
    (cmd: string, args: object, done: (e: string | null, v: unknown) => void) => {
      const tauri = (
        window as unknown as {
          __TAURI__: { core: { invoke: (c: string, a: object) => Promise<unknown> } };
        }
      ).__TAURI__;
      tauri.core.invoke(cmd, args).then((v) => done(null, v), (e) => done(String(e), null));
    },
    cmd,
    args,
  );

const appProcesses = (): string =>
  execSync("ps -eo comm,args | grep -E '/fotobase( |$)' | grep -v grep || true", {
    encoding: "utf8",
  });

describe("tray background watching", () => {
  before(async () => {
    lib = makeLibrary();
    await browser.waitUntil(async () =>
      browser.execute(() => Boolean((window as unknown as { __TAURI__?: unknown }).__TAURI__)),
    );
    const error = await invoke("set_folder_mode", { path: lib.dir, mode: "always" });
    expect(error).toBeNull();
  });

  it("keeps the process and the watcher alive when the window closes", async () => {
    // Close the window (the tray intercepts the request and hides it).
    const error = await browser.executeAsync((done: (e: string | null) => void) => {
      const tauri = (
        window as unknown as {
          __TAURI__: {
            window: { getCurrentWindow: () => { close: () => Promise<void> } };
          };
        }
      ).__TAURI__;
      tauri.window.getCurrentWindow().close().then(() => done(null), (e) => done(String(e)));
    });
    expect(error).toBeNull();
    // The app is still alive: the session survives and IPC answers.
    await browser.waitUntil(async () => (await invoke("ping")) === "pong", {
      timeoutMsg: "app stopped answering IPC after window close",
    });
    expect(appProcesses()).toContain("fotobase");
  });

  it("picks up new files while the window is closed", async () => {
    const fresh = path.join(lib.dir, "while-closed.jpg");
    fs.writeFileSync(fresh, Buffer.alloc(512, 0xab));
    // The catalog is the queryable side effect: resolve the folder id, then
    // wait for the watched rescan to list the new file.
    await browser.waitUntil(async () => {
      const folders = (await invoke("list_folders")) as { id: number; path: string }[];
      const id = folders.find((f) => f.path === lib.dir)?.id;
      if (id === undefined) return false;
      const files = (await invoke("list_files", { folders: [id] })) as { filename: string }[];
      return files.some((f) => f.filename === "while-closed.jpg");
    }, { timeoutMsg: "watcher missed a file written while the window was closed" });
  });

  it("quit fully exits the process", async () => {
    // Same function the tray menu's Quit item runs (tray::quit); the OS
    // tray itself is outside WebDriver's reach.
    await invoke("quit_from_tray");
    await browser.waitUntil(() => appProcesses().trim() === "", {
      timeout: 10_000,
      timeoutMsg: "fotobase process still alive after tray quit",
    });
  });
});
