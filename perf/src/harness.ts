#!/usr/bin/env tsx
// TASK-026 harness runner (docs/webkitgtk-perf-protocol.md is normative).
//
// Two modes:
//
//   --self-check      Meta test from the task's Testing criteria: build a
//                     synthetic report whose first-thumb metric carries an
//                     injected +5 s delay, report the miss, exit non-zero.
//                     Needs no app, no fixture, no WebDriver.
//
//   (default)         Full protocol run: cold-start the app against a fresh
//                     data dir, hand it the 00-013 fixture as a watched
//                     folder (FOTOBASE_PERF_FIXTURE), watch the §2 metrics
//                     come in through the `perf_probe` IPC command, script
//                     the grid scroll via WDIO, sample the process-tree
//                     RSS, and emit `perf-report.json`.
//
//   --smoke           CI mode (protocol §5): same full run, but the exit
//                     code only reflects whether a schema-valid report was
//                     produced — the §2 bars are never enforced on CI
//                     hardware (it is not the decision hardware).

import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import { createServer } from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { remote } from "webdriverio";

import {
  buildReport,
  evaluateBars,
  selfCheckMetrics,
  validateReport,
  type Metrics,
  type Renderer,
} from "./report.js";

// --- CLI --------------------------------------------------------------------

interface Args {
  selfCheck: boolean;
  smoke: boolean;
  fixture: string;
  count: number;
  seed: number;
  app: string;
  renderer: Renderer;
  out: string;
  scrollS: number;
  runs: number;
  quiesceS: number;
  timeoutS: number;
}

function parseArgs(argv: string[]): Args {
  const get = (flag: string): string | undefined => {
    const i = argv.indexOf(flag);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const has = (flag: string): boolean => argv.includes(flag);
  const repoRoot = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    "..",
    "..",
  );
  return {
    selfCheck: has("--self-check"),
    smoke: has("--smoke"),
    fixture: get("--fixture") ?? "",
    count: Number(get("--count") ?? 1000),
    seed: Number(get("--seed") ?? 42),
    app: get("--app") ?? path.join(repoRoot, "target", "debug", "fotobase"),
    renderer: (get("--renderer") as Renderer | undefined) ?? "dom",
    out: get("--out") ?? "perf-report.json",
    scrollS: Number(get("--scroll-s") ?? 10),
    runs: Number(get("--runs") ?? 5),
    quiesceS: Number(get("--quiesce-s") ?? 30),
    timeoutS: Number(get("--timeout-s") ?? 1800),
  };
}

// --- Self-check (the task's meta test) --------------------------------------

function runSelfCheck(): number {
  const metrics = selfCheckMetrics();
  const report = buildReport({
    fixture: { count: 1000, seed: 42 },
    runner: { os: "self-check", webkitgtk: "n/a", cpu: "n/a", ram_gb: 0 },
    renderer: "dom",
    metrics,
  });
  console.log(JSON.stringify(report, null, 2));
  const failures = evaluateBars(metrics);
  for (const f of failures) console.error(`MISSED BAR: ${f}`);
  console.error(
    failures.length > 0 ? "self-check: MISS (exit 1)" : "self-check: PASS",
  );
  return report.passed ? 0 : 1;
}

// --- Runner info (§3: stable environment notes) ------------------------------

function runnerInfo(): {
  os: string;
  webkitgtk: string;
  cpu: string;
  ram_gb: number;
} {
  let webkitgtk = "n/a";
  if (process.platform === "linux") {
    for (const pkg of ["webkit2gtk-4.1", "webkit2gtk-4.0"]) {
      try {
        webkitgtk = execFileSync("pkg-config", ["--modversion", pkg], {
          encoding: "utf8",
        }).trim();
        break;
      } catch {
        /* try the next pkg-config name */
      }
    }
  } else if (process.platform === "darwin") {
    webkitgtk = `wkwebview ${os.release()}`;
  }
  return {
    os: `${os.platform()} ${os.release()}`,
    webkitgtk,
    cpu: os.cpus()[0]?.model ?? "unknown",
    ram_gb: Math.round(os.totalmem() / 2 ** 30),
  };
}

// --- Process-tree RSS sampling (§2: app process tree RSS) --------------------

class RssSampler {
  private samples: number[] = [];
  private peak = 0;
  private timer: NodeJS.Timeout | undefined;
  private appPid = 0;

  start(binaryName: string, intervalMs = 500): void {
    this.timer = setInterval(() => {
      try {
        const pid = this.appPid || (this.appPid = findAppPid(binaryName));
        if (!pid) return;
        const kb = treeRssKb(pid);
        if (kb > 0) {
          const mb = kb / 1024;
          this.samples.push(mb);
          this.peak = Math.max(this.peak, mb);
        }
      } catch {
        /* sampling is best-effort; the report degrades gracefully */
      }
    }, intervalMs);
    this.timer.unref?.();
  }

  stop(): void {
    if (this.timer) clearInterval(this.timer);
  }

  /** Median of the samples collected in the last `windowMs` (idle RAM). */
  idleMedian(windowMs: number): number {
    const recent = this.samples.slice(-Math.max(1, Math.ceil(windowMs / 500)));
    return median(recent);
  }

  get peakMb(): number {
    return this.peak;
  }
}

function findAppPid(binaryName: string): number {
  try {
    const out = execFileSync("pgrep", ["-f", binaryName], { encoding: "utf8" });
    return Number(out.trim().split("\n")[0]) || 0;
  } catch {
    return 0;
  }
}

function treeRssKb(pid: number): number {
  if (process.platform === "linux") {
    const own = Number(readProcField(pid, "VmRSS:"));
    let total = Number.isFinite(own) ? own : 0;
    // Children (WebKitWebProcess / WebKitNetworkProcess) count toward the
    // app's footprint — the protocol measures the app process tree.
    for (const other of listPids()) {
      if (other !== pid && parentPid(other) === pid) {
        const kb = Number(readProcField(other, "VmRSS:"));
        if (Number.isFinite(kb)) total += kb;
      }
    }
    return total;
  }
  if (process.platform === "darwin") {
    // `ps -axo pid=,ppid=,rss=` — RSS in 1 KiB units; walk the same tree.
    const rows = execFileSync("ps", ["-axo", "pid=,ppid=,rss="], {
      encoding: "utf8",
    })
      .trim()
      .split("\n")
      .map((line) => line.trim().split(/\s+/).map(Number))
      .filter((r) => r.length >= 3 && Number.isFinite(r[0]));
    const byPid = new Map(rows.map((r) => [r[0], r]));
    let total = 0;
    const stack = [pid];
    while (stack.length > 0) {
      const cur = stack.pop()!;
      const row = byPid.get(cur);
      if (!row) continue;
      total += row[2];
      for (const r of rows) if (r[1] === cur && r[0] !== pid) stack.push(r[0]);
    }
    return total;
  }
  return 0; // Windows: not wired (CI smoke runs on Linux) — reported as 0.
}

function listPids(): number[] {
  return fs
    .readdirSync("/proc")
    .filter((d) => /^\d+$/.test(d))
    .map(Number);
}

function parentPid(pid: number): number {
  try {
    // /proc/<pid>/stat field 4 is the ppid; the comm field can contain
    // spaces, so split after the last ')'.
    const stat = fs.readFileSync(`/proc/${pid}/stat`, "utf8");
    return Number(stat.slice(stat.lastIndexOf(")") + 2).split(" ")[1]);
  } catch {
    return 0;
  }
}

function readProcField(pid: number, field: string): number {
  try {
    const status = fs.readFileSync(`/proc/${pid}/status`, "utf8");
    const line = status.split("\n").find((l) => l.startsWith(field));
    return Number(line?.split(/\s+/)[1]);
  } catch {
    return NaN;
  }
}

function median(xs: number[]): number {
  if (xs.length === 0) return 0;
  const s = [...xs].sort((a, b) => a - b);
  const mid = Math.floor(s.length / 2);
  return s.length % 2 === 1 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

// --- In-page measurement scripts ---------------------------------------------

/** rAF stall monitor: counts main-thread gaps > 100 ms (§2 ui_jank_events). */
const JANK_MONITOR = `
  window.__fbJank = { gaps: 0, last: performance.now() };
  const loop = (now) => {
    if (now - window.__fbJank.last > 100) window.__fbJank.gaps += 1;
    window.__fbJank.last = now;
    window.__fbJank.raf = requestAnimationFrame(loop);
  };
  window.__fbJank.raf = requestAnimationFrame(loop);
  true;
`;

const JANK_READ = `
  const j = window.__fbJank;
  if (j && j.raf) cancelAnimationFrame(j.raf);
  const gaps = j ? j.gaps : 0;
  window.__fbJank = undefined;
  gaps;
`;

/** Scripted grid scroll: ping-pong `scrollTop` steps while counting rAF
 * frames over the run window (§2 scroll_fps). Works for both renderers —
 * each mounts its scroller under a `data-testid` the harness probes. */
const SCROLL_RUN = `
  const [durationMs, tickMs, stepPx, done] = arguments;
  const el =
    document.querySelector('[data-testid="grid-scroll"]') ||
    document.querySelector('[data-testid="canvas-grid-scroll"]');
  if (!el) { done({ error: "no grid scroller found" }); return; }
  let frames = 0;
  let jank = 0;
  let last = performance.now();
  const t0 = last;
  let dir = 1;
  const wheel = setInterval(() => {
    const max = el.scrollHeight - el.clientHeight;
    if (el.scrollTop >= max) dir = -1;
    if (el.scrollTop <= 0) dir = 1;
    el.scrollTop += dir * stepPx;
  }, tickMs);
  const loop = (now) => {
    if (now - last > 100) jank += 1;
    frames += 1;
    last = now;
    if (now - t0 < durationMs) { requestAnimationFrame(loop); }
    else { clearInterval(wheel); done({ frames, elapsedMs: now - t0, jank }); }
  };
  requestAnimationFrame(loop);
`;

async function waitUntil(
  fn: () => Promise<boolean>,
  timeoutMs: number,
  everyMs = 250,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await fn()) return;
    await sleep(everyMs);
  }
  throw new Error(`timeout after ${timeoutMs} ms`);
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

// --- Full protocol run --------------------------------------------------------

interface Probe {
  scan_started_ms: number;
  scan_completed_ms: number | null;
  first_thumb_serve_ms: number | null;
  expected: number;
}

async function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const srv = createServer();
    srv.listen(0, "127.0.0.1", () => {
      const addr = srv.address();
      if (addr && typeof addr === "object") {
        srv.close(() => resolve(addr.port));
      } else {
        srv.close();
        reject(new Error("no port"));
      }
    });
    srv.on("error", reject);
  });
}

function appDataDir(home: string): string {
  switch (process.platform) {
    case "linux":
      return path.join(home, ".local", "share", "io.github.fotobase.app");
    case "darwin":
      return path.join(
        home,
        "Library",
        "Application Support",
        "io.github.fotobase.app",
      );
    default:
      return path.join(home, "AppData", "Roaming", "io.github.fotobase.app");
  }
}

async function runFull(args: Args): Promise<number> {
  if (process.platform === "win32") {
    console.error(
      "The full run needs tauri-driver; Windows (msedgedriver) is not wired up. Run on Linux/macOS.",
    );
    return 2;
  }
  if (!args.fixture || !fs.existsSync(args.fixture)) {
    console.error(`--fixture directory not found: ${args.fixture}`);
    console.error(
      "Generate it first: python3 tools/fixture_gen.py --count <N> --seed 42 --out <dir>",
    );
    return 2;
  }
  if (!fs.existsSync(args.app)) {
    console.error(
      `--app binary not found: ${args.app} (cargo build -p fotobase first)`,
    );
    return 2;
  }

  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "fb-perf-"));
  const home = path.join(tmp, "home");
  const dataDir = appDataDir(home);
  fs.mkdirSync(dataDir, { recursive: true });
  // The renderer under test is pinned through the app's own settings store.
  fs.writeFileSync(
    path.join(dataDir, "settings.json"),
    JSON.stringify({ grid_renderer: args.renderer }),
  );

  const port = await freePort();
  // tauri-driver renamed its listen flag between v1 (--webdriver-port) and
  // v2 (--port); pick the one the installed binary actually accepts.
  const driverFlag = execFileSync("tauri-driver", ["--help"], {
    encoding: "utf8",
  }).includes("--webdriver-port")
    ? "--webdriver-port"
    : "--port";
  // The app inherits the driver's environment: a fresh HOME gives the
  // protocol §1 cold start (empty catalog + empty thumbnail cache), and the
  // FOTOBASE_PERF_* variables arm the perf hook inside the app.
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    HOME: home,
    XDG_DATA_HOME: path.join(home, ".local", "share"),
    FOTOBASE_PERF_FIXTURE: path.resolve(args.fixture),
    FOTOBASE_PERF_COUNT: String(args.count),
  };
  const driver = spawn("tauri-driver", [driverFlag, String(port)], {
    stdio: ["ignore", "inherit", "inherit"],
    env,
  });

  let browser: Awaited<ReturnType<typeof remote>> | undefined;
  const sampler = new RssSampler();
  const startedAt = Date.now();
  try {
    browser = await remote({
      hostname: "127.0.0.1",
      port,
      logLevel: "warn",
      // Single object, not an array: webdriverio v9 passes `capabilities`
      // straight through as alwaysMatch, and WebKitWebDriver rejects a
      // JSON array there ("alwaysMatch is invalid in capabilities").
      capabilities: {
        browserName: "wry",
        "tauri:options": { application: path.resolve(args.app) },
      },
    });

    sampler.start(path.basename(args.app));

    // The app boots, arms the fixture folder (env hook), and the scan starts.
    await waitUntil(
      async () =>
        Boolean(
          await browser!.execute(() => Boolean((window as any).__TAURI__)),
        ),
      60_000,
    );

    // §2 jank during scan: install the rAF stall monitor before the scan
    // fills the catalog.
    await browser.execute(JANK_MONITOR);

    // Read the probe until the scan completes (the hook thread marks it).
    let probe: Probe | undefined;
    await waitUntil(async () => {
      probe = (await browser!.execute(() =>
        (window as any).__TAURI__.core.invoke("perf_probe"),
      )) as Probe | undefined;
      return Boolean(probe?.scan_completed_ms);
    }, args.timeoutS * 1000);
    const p = probe as Probe;

    const scanJank = (await browser.execute(JANK_READ)) as number;

    // §2 time_to_first_thumbnail: scan start → first real thumbnail served.
    // A scan that finishes without ever serving a real thumbnail is a miss,
    // not a harness error: report the elapsed time and let the bars decide.
    let firstThumbS = p.first_thumb_serve_ms
      ? (p.first_thumb_serve_ms - p.scan_started_ms) / 1000
      : (Date.now() - startedAt) / 1000;
    if (!p.first_thumb_serve_ms) {
      console.error(
        "warning: no real thumbnail was served during the run; reporting elapsed time",
      );
      firstThumbS = Math.max(firstThumbS, args.timeoutS);
    }

    // §2 scroll_fps: median of N scripted 10 s runs; jank accumulates.
    const fpsRuns: number[] = [];
    let scrollJank = 0;
    for (let i = 0; i < args.runs; i++) {
      await browser.execute((sel) => {
        const el = document.querySelector(sel);
        if (el) el.scrollTop = 0;
      }, '[data-testid="grid-scroll"], [data-testid="canvas-grid-scroll"]');
      const run = (await browser.executeAsync(
        SCROLL_RUN,
        args.scrollS * 1000,
        50,
        600,
      )) as {
        frames: number;
        elapsedMs: number;
        jank: number;
        error?: string;
      };
      if (run.error) throw new Error(run.error);
      fpsRuns.push(run.frames / (run.elapsedMs / 1000));
      scrollJank += run.jank;
      console.error(
        `scroll run ${i + 1}/${args.runs}: ${fpsRuns[fpsRuns.length - 1].toFixed(1)} fps`,
      );
    }

    // §2 idle RAM: quiesce after the full scan, then take the median of the
    // tail samples; the sampler tracked the peak across the whole session.
    console.error(`quiescing ${args.quiesceS} s before the idle RAM reading…`);
    await sleep(args.quiesceS * 1000);
    const idleRamMb = sampler.idleMedian(10_000);

    const metrics: Metrics = {
      time_to_first_thumbnail_s: round1(firstThumbS),
      full_scan_duration_s: round1(
        (p.scan_completed_ms! - p.scan_started_ms) / 1000,
      ),
      scroll_fps_median: round1(median(fpsRuns)),
      idle_ram_mb: round1(idleRamMb),
      ui_jank_events: scanJank + scrollJank,
      peak_ram_mb: round1(sampler.peakMb),
    };

    await browser.deleteSession();
    browser = undefined;
    sampler.stop();

    const report = buildReport({
      fixture: { count: args.count, seed: args.seed },
      runner: runnerInfo(),
      renderer: args.renderer,
      metrics,
    });
    const json = JSON.stringify(report, null, 2);
    fs.writeFileSync(args.out, json + "\n");
    console.log(json);

    const failures = evaluateBars(metrics);
    for (const f of failures) console.error(`MISSED BAR: ${f}`);

    if (args.smoke) {
      // Protocol §5: CI asserts production + schema validity, never the bars
      // (CI hardware is not the decision hardware).
      const schemaErrors = validateReport(report);
      if (schemaErrors.length > 0) {
        for (const e of schemaErrors) console.error(`SCHEMA: ${e}`);
        return 1;
      }
      console.error(
        "smoke: schema-valid perf-report.json produced (bars not enforced in CI)",
      );
      return 0;
    }
    return report.passed ? 0 : 1;
  } catch (e) {
    console.error(
      `harness error: ${e instanceof Error ? e.message : String(e)}`,
    );
    return 2;
  } finally {
    if (browser) await browser.deleteSession().catch(() => {});
    sampler.stop();
    driver.kill("SIGTERM");
    // Leave the temp home for post-mortem (catalog/cache), but bounded.
    setTimeout(
      () => fs.rmSync(tmp, { recursive: true, force: true }),
      1000,
    ).unref?.();
  }
}

function round1(n: number): number {
  return Math.round(n * 10) / 10;
}

// --- main ---------------------------------------------------------------------

const args = parseArgs(process.argv.slice(2));
if (args.selfCheck) {
  process.exit(runSelfCheck());
} else {
  runFull(args).then((code) => process.exit(code));
}
