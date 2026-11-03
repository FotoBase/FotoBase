// WebdriverIO + tauri-driver (PLAN §4): drives the real Tauri app through
// WebKitWebDriver. tauri-driver supports Linux and Windows only (macOS has
// no WKWebView driver), so the run happens in CI on Ubuntu (.github/
// workflows/e2e.yml) or in a Linux container. The app starts with a fresh
// HOME, so every run is a cold start: empty catalog, empty thumbnail cache,
// default settings.

import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { browser } from "@wdio/globals";

import { withDeadline } from "./support/interact";
import { APP, DRIVER, NATIVE_DRIVER, describeProcs, homeOf, listProcs, matching, reap, threadStacksToFile } from "./support/procs";

const repoRoot = path.resolve(import.meta.dirname, "..");
const appBinary =
  process.env.FOTOBASE_E2E_APP ?? path.join(repoRoot, "target", "debug", "fotobase");
const DRIVER_PORT = 4444;

let driver: ChildProcess | undefined;
let home: string | undefined;
let stacksDumped = false;

// Thread backtraces of this session's app and its web-content process into
// the uploaded artifact (TASK-05-013). The page's JS runs in
// WebKitWebProcess, so that is the stack that names a runaway script.
function dumpAppStacks(label: string): void {
  const mine = matching(listProcs(), /\/fotobase( |$)|WebKitWebProcess/).filter(
    (p) => homeOf(p.pid) === home,
  );
  if (mine.length === 0) {
    // eslint-disable-next-line no-console
    console.log(`[e2e] stack dump: no app process for this session; all: ${describeProcs(matching(listProcs(), APP))}`);
    return;
  }
  const dir = path.join(import.meta.dirname, "screenshots");
  fs.mkdirSync(dir, { recursive: true });
  for (const p of mine) {
    const file = path.join(dir, `${label}-stacks-${p.pid}.txt`);
    threadStacksToFile(p.pid, file);
    // eslint-disable-next-line no-console
    console.log(`[e2e] stack dump started: ${p.args.split(" ")[0]} pid=${p.pid} -> ${path.basename(file)}`);
  }
}

function driverListenFlag(): string {
  // tauri-driver renamed its listen flag between v1 and v2.
  const help = execFileSync("tauri-driver", ["--help"], { encoding: "utf8" });
  return help.includes("--webdriver-port") ? "--webdriver-port" : "--port";
}

async function portOpen(port: number): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    const socket = net.connect(port, "127.0.0.1");
    socket.once("connect", () => {
      socket.destroy();
      resolve(true);
    });
    socket.once("error", () => resolve(false));
  });
}

// WebKitWebDriver (spawned by tauri-driver) takes DRIVER_PORT + 1, so both
// must be free before a new driver starts — otherwise the new driver fails
// to bind while wdio happily connects to the stale one from the previous
// spec file, driving the wrong app instance.
const ALL_DRIVER_PORTS = [DRIVER_PORT, DRIVER_PORT + 1];

async function waitForPort(port: number, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await portOpen(port)) return;
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`tauri-driver did not start listening on port ${DRIVER_PORT}`);
}

async function waitPortsFree(timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const busy = (await Promise.all(ALL_DRIVER_PORTS.map(portOpen))).some(Boolean);
    if (!busy) return;
    if (Date.now() > deadline) {
      throw new Error(
        `ports ${ALL_DRIVER_PORTS.join("/")} still occupied by a stale driver`,
      );
    }
    await new Promise((r) => setTimeout(r, 200));
  }
}

// Kill every driver and app instance (maxInstances is 1, so all of them are
// leftovers at this point) and log what had to be killed: a leftover is the
// first thing to look at when the next spec file misbehaves. Shell-free —
// see support/procs.ts for why `pkill -f` in execSync never worked here.
async function reapAll(phase: string): Promise<void> {
  for (const [label, pattern] of [
    ["app", APP],
    ["WebKitWebDriver", NATIVE_DRIVER],
    ["tauri-driver", DRIVER],
  ] as const) {
    const killed = await reap(pattern);
    if (killed.length > 0) {
      // eslint-disable-next-line no-console
      console.log(
        `[e2e] ${phase}: reaped ${label}: ${killed
          .map((k) => `pid=${k.pid} (${k.signal}) etime=${k.etime} ${k.args}`)
          .join("; ")}`,
      );
    }
  }
}

export const config: WebdriverIO.Config = {
  runner: "local",
  specs: ["./specs/**/*.e2e.ts"],
  maxInstances: 1,
  hostname: "127.0.0.1",
  port: DRIVER_PORT,
  logLevel: "warn",
  waitforTimeout: 30_000,
  connectionRetryTimeout: 120_000,
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: { ui: "bdd", timeout: 180_000 },
  // WebKitGTK under xvfb (software GL) intermittently hangs a webview
  // mid-spec; a retry in a fresh session (driver, HOME, library) is the
  // bounded answer. Safe now that sessions can no longer share a driver.
  specFileRetries: 1,
  specFileRetriesDelay: 5,
  capabilities: [
    {
      // webdriverio v9 otherwise injects `webSocketUrl: true` (BiDi), which
      // WebKitWebDriver rejects: "alwaysMatch is invalid in capabilities".
      "wdio:enforceWebDriverClassic": true,
      browserName: "wry",
      "tauri:options": { application: appBinary },
    } as WebdriverIO.Capabilities,
  ],

  onPrepare() {
    if (process.platform === "darwin") {
      throw new Error(
        "tauri-driver has no macOS support; run the e2e suite on Linux (CI or a container).",
      );
    }
    if (!fs.existsSync(appBinary)) {
      throw new Error(
        `app binary not found: ${appBinary} (cargo build -p fotobase --features tauri/custom-protocol first)`,
      );
    }
  },

  async beforeSession(_config, _capabilities, specs, cid) {
    // A previous session's driver or app can outlive it (a wedged webview
    // never answers DELETE /session). Clear them out and wait for the ports
    // to be released, or the new driver fails to bind and the session
    // silently attaches to the stale one.
    await reapAll("beforeSession");
    await waitPortsFree(30_000).catch(async (e: Error) => {
      // eslint-disable-next-line no-console
      console.log(`[e2e] beforeSession: ${e.message}; reaping again`);
      await reapAll("beforeSession (ports busy)");
      return waitPortsFree(10_000);
    });
    home = fs.mkdtempSync(path.join(os.tmpdir(), "fb-e2e-home-"));
    stacksDumped = false;
    // Specs read this to tell their own app instance from leftovers.
    process.env.FB_E2E_HOME = home;
    // The app inherits the driver's environment.
    driver = spawn("tauri-driver", [driverListenFlag(), String(DRIVER_PORT)], {
      stdio: ["ignore", "inherit", "inherit"],
      env: {
        ...process.env,
        HOME: home,
        XDG_DATA_HOME: path.join(home, ".local", "share"),
        XDG_CONFIG_HOME: path.join(home, ".config"),
      },
    });
    await waitForPort(DRIVER_PORT, 30_000);
    // Something answered on the port — make sure it is OUR driver, not a
    // stale one while ours died failing to bind.
    if (driver.exitCode !== null || driver.signalCode !== null) {
      throw new Error(
        `tauri-driver exited (${driver.exitCode ?? driver.signalCode}) right after start; port ${DRIVER_PORT} is held by another process`,
      );
    }
    // eslint-disable-next-line no-console
    console.log(
      `[e2e] session start ${cid}: ${specs.map((s) => path.basename(s)).join(", ")} driver pid=${driver.pid} HOME=${home}`,
    );
  },

  // A failing step leaves a screenshot of the app (uploaded by the workflow).
  // Bounded and non-fatal: on a crashed or wedged webview the screenshot
  // itself fails or hangs, and that must not bury the test's own error.
  async afterTest(test, _context, { passed }) {
    if (passed) return;
    fs.mkdirSync(path.join(import.meta.dirname, "screenshots"), { recursive: true });
    const name = test.title.replace(/[^a-z0-9]+/gi, "-").toLowerCase();
    try {
      await withDeadline(
        browser.saveScreenshot(path.join(import.meta.dirname, "screenshots", `${name}.png`)),
        20_000,
        "screenshot",
      );
    } catch (e) {
      // eslint-disable-next-line no-console
      console.log(`[e2e] no screenshot for "${test.title}": ${String(e).split("\n")[0]}`);
      // The webview did not answer: capture where the app is stuck, once
      // per session (TASK-05-013).
      if (!stacksDumped) {
        stacksDumped = true;
        dumpAppStacks(name);
      }
    }
  },

  async afterSession() {
    if (driver) {
      driver.kill();
      driver = undefined;
    }
    // WebKitWebDriver survives a SIGTERM to tauri-driver, and the app
    // (spawned by the driver, not our child) survives both when its webview
    // is wedged. Anything still alive here is logged and killed.
    await reapAll("afterSession");
    if (process.env.FB_E2E_HOME === home) delete process.env.FB_E2E_HOME;
    // The app may still be flushing its cache as it exits: retry, never fail the run.
    if (home) {
      try {
        fs.rmSync(home, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
      } catch {
        // a leftover temp dir is harmless
      }
    }
    home = undefined;
  },
};
