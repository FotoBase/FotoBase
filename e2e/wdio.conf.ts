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

async function waitForPort(port: number, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const open = await new Promise<boolean>((resolve) => {
      const socket = net.connect(port, "127.0.0.1");
      socket.once("connect", () => {
        socket.destroy();
        resolve(true);
      });
      socket.once("error", () => resolve(false));
    });
    if (open) return;
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`tauri-driver did not start listening on port ${DRIVER_PORT}`);
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
    home = fs.mkdtempSync(path.join(os.tmpdir(), "fb-e2e-home-"));
    // The app inherits the driver's environment.
    driver = spawn("tauri-driver", [driverListenFlag(), String(DRIVER_PORT)], {
      stdio: ["ignore", "inherit", "inherit"],
      env: {
        ...process.env,
        HOME: home,
        XDG_DATA_HOME: path.join(home, ".local", "share"),
        XDG_CONFIG_HOME: path.join(home, ".config"),
        // Tray spec: WebKitWebDriver cannot execute against a hidden view,
        // so the tray's close-to-tray interception keeps the window mapped
        // under the driver (the app still does not exit).
        FOTOBASE_E2E_KEEP_WINDOW: "1",
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
    driver?.kill();
    driver = undefined;
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
