// WebdriverIO + tauri-driver (PLAN §4): drives the real Tauri app through
// WebKitWebDriver. tauri-driver supports Linux and Windows only (macOS has
// no WKWebView driver), so the run happens in CI on Ubuntu (.github/
// workflows/e2e.yml) or in a Linux container. The app starts with a fresh
// HOME, so every run is a cold start: empty catalog, empty thumbnail cache,
// default settings.

import { execFileSync, execSync, spawn, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { browser } from "@wdio/globals";

const repoRoot = path.resolve(import.meta.dirname, "..");
const appBinary =
  process.env.FOTOBASE_E2E_APP ?? path.join(repoRoot, "target", "debug", "fotobase");
const DRIVER_PORT = 4444;

let driver: ChildProcess | undefined;
let home: string | undefined;

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

  async beforeSession() {
    // The previous session's driver may not be fully gone yet (kill is async
    // and WebKitWebDriver outlives tauri-driver). Wait for the ports to be
    // released, or the new driver fails to bind and the session silently
    // attaches to the stale one.
    await waitPortsFree(30_000).catch(() => {
      // A stale driver that never lets go is fatal for correctness: kill it.
      execSync("pkill -f tauri-driver || true; pkill -f WebKitWebDriver || true");
      return waitPortsFree(10_000);
    });
    home = fs.mkdtempSync(path.join(os.tmpdir(), "fb-e2e-home-"));
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
  },

  // A failing step leaves a screenshot of the app (uploaded by the workflow).
  async afterTest(test, _context, { passed }) {
    if (passed) return;
    fs.mkdirSync(path.join(import.meta.dirname, "screenshots"), { recursive: true });
    const name = test.title.replace(/[^a-z0-9]+/gi, "-").toLowerCase();
    await browser.saveScreenshot(path.join(import.meta.dirname, "screenshots", `${name}.png`));
  },

  afterSession() {
    if (driver) {
      // Kill the whole tree: WebKitWebDriver (the child tauri-driver spawns,
      // holding DRIVER_PORT + 1) survives a plain SIGTERM to its parent and
      // would poison the next session's port allocation.
      try {
        execSync(`pkill -P ${driver.pid} || true`);
      } catch {
        // no children — fine
      }
      driver.kill();
      driver = undefined;
    }
    // Belt and braces for the app itself (spawned by the driver, so not our
    // child): a leftover instance would keep ports busy and HOME stale.
    try {
      execSync("pkill -f tauri-driver || true; pkill -f WebKitWebDriver || true; pkill -f '/fotobase( |$)' || true");
    } catch {
      // nothing left to kill — fine
    }
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
