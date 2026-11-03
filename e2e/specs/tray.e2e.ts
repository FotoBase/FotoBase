// TASK-03-025 e2e: tray/background watching, end to end against the real
// app. Closing the main window must keep the process (and the watcher)
// alive; a file dropped into a watched folder while the window is closed
// still lands in the catalog; and quitting (the exact code path the tray
// menu's Quit item runs — WebDriver cannot click the OS tray, so the spec
// exposes the same `tray::quit` as the `quit_from_tray` command) fully
// exits with no orphan process.
//
// The close-to-tray interception is driven through the `close_main_window`
// IPC command (tray::close_intercepted minus the hide): WebKitWebDriver
// cannot execute against a hidden (unmapped) view, so the spec's window
// stays mapped while the interception and background-watching behavior are
// still what the spec proves.

import { browser, expect } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";

import { makeLibrary, type Library } from "../support/library";
import { APP, describeProcs, listProcs, matching } from "../support/procs";

let lib: Library;

type Reply = { ok: true; value: unknown } | { ok: false; error: string };

// WebDriver's async-script callback carries ONE result, so the reply is
// wrapped; a rejected command throws here.
const invoke = async (cmd: string, args: object = {}): Promise<unknown> => {
  const reply = await browser.executeAsync<Reply, [string, object]>(
    (cmd: string, args: object, done: (r: Reply) => void) => {
      const tauri = (
        window as unknown as {
          __TAURI__: {
            core: { invoke: (c: string, a: object) => Promise<unknown> };
          };
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
  if (!reply.ok) throw new Error(`${cmd} failed: ${reply.error}`);
  return reply.value;
};

const appProcesses = () => matching(listProcs(), APP);

describe("tray background watching", () => {
  before(async () => {
    lib = makeLibrary();
    await browser.waitUntil(async () =>
      browser.execute(() =>
        Boolean((window as unknown as { __TAURI__?: unknown }).__TAURI__),
      ),
    );
    const error = await invoke("set_folder_mode", {
      path: lib.dir,
      mode: "always",
    });
    expect(error).toBeNull();
  });

  it("keeps the process and the watcher alive when the window closes", async () => {
    // Close the window through the interception itself. The JS window
    // `close()` API cannot be used here: on Linux/WebKitGTK it destroys
    // the webview even when the close request is prevented
    // (tauri-apps/tauri#8435), killing the app mid-test.
    const error = await invoke("close_main_window");
    expect(error).toBeNull();
    // The app is still alive: the session survives and IPC answers.
    await browser.waitUntil(async () => (await invoke("ping")) === "pong", {
      timeoutMsg: "app stopped answering IPC after window close",
    });
    expect(appProcesses().length).toBeGreaterThan(0);
  });

  it("picks up new files while the window is closed", async () => {
    const fresh = path.join(lib.dir, "while-closed.jpg");
    fs.writeFileSync(fresh, Buffer.alloc(512, 0xab));
    // The catalog is the queryable side effect: resolve the folder id, then
    // wait for the watched rescan to list the new file. IPC errors are
    // captured so a failure names the actual error, not "null has no find".
    let ipcError = "";
    await browser.waitUntil(
      async () => {
        const folders = (await invoke("list_folders")) as
          | { id: number; path: string }[]
          | null;
        if (!folders) {
          ipcError = "list_folders failed";
          return false;
        }
        const id = folders.find((f) => f.path === lib.dir)?.id;
        if (id === undefined) return false;
        const files = (await invoke("list_files", { folders: [id] })) as
          | { filename: string }[]
          | null;
        if (!files) {
          ipcError = "list_files failed";
          return false;
        }
        return files.some((f) => f.filename === "while-closed.jpg");
      },
      {
        timeoutMsg: `watcher missed a file written while the window was closed${ipcError ? ` (${ipcError})` : ""}`,
      },
    );
  });

  it("quit fully exits the process", async () => {
    // Same function the tray menu's Quit item runs (tray::quit); the OS
    // tray itself is outside WebDriver's reach.
    // Fire-and-forget, deferred: the execute must return before the app
    // starts exiting, or the exit races the reply and the command fails
    // with "invalid session id" (TASK-05-013).
    const before = appProcesses();
    const started = Date.now();
    await browser.execute(() => {
      const core = (
        window as unknown as {
          __TAURI__: { core: { invoke: (c: string) => Promise<unknown> } };
        }
      ).__TAURI__.core;
      setTimeout(() => void core.invoke("quit_from_tray"), 100);
    });
    try {
      await browser.waitUntil(() => appProcesses().length === 0, { timeout: 10_000 });
    } catch {
      // Name what is still alive and whose it is (this session's app vs a
      // leftover from an earlier spec file): TASK-05-013.
      throw new Error(
        `fotobase process still alive after tray quit; before quit: ${describeProcs(before)}; now: ${describeProcs(appProcesses())}`,
      );
    }
    // eslint-disable-next-line no-console
    console.log(`[e2e] tray quit: process gone after ${Date.now() - started}ms`);
  });
});
