// Process helpers for the e2e harness (TASK-05-013). Shell-free on purpose:
// `execSync("pkill -f tauri-driver || true; …")` runs under `sh -c "<that
// string>"`, and Linux `pkill -f` matches — and kills — that very shell, so
// every later kill in the chain silently never ran. Stale drivers and app
// instances then leaked into the next spec file (port race, "fotobase
// process still alive after tray quit"). Listing with `ps` (no shell) and
// filtering in JS cannot match ourselves.

import { execFileSync } from "node:child_process";
import fs from "node:fs";

export interface Proc {
  pid: number;
  ppid: number;
  stat: string;
  etime: string;
  args: string;
}

export interface Killed extends Proc {
  signal: "SIGTERM" | "SIGKILL";
}

/// The app binary (`…/fotobase`, optionally with arguments). A zombie shows
/// as `[fotobase] <defunct>` and is not matched: it has already exited.
export const APP = /\/fotobase( |$)/;
export const DRIVER = /tauri-driver/;
export const NATIVE_DRIVER = /WebKitWebDriver/;

const PS_LINE = /^\s*(\d+)\s+(\d+)\s+(\S+)\s+(\S+)\s+(.*)$/;

export function parsePs(out: string): Proc[] {
  const procs: Proc[] = [];
  for (const line of out.split("\n")) {
    const m = PS_LINE.exec(line);
    if (!m) continue;
    procs.push({
      pid: Number(m[1]),
      ppid: Number(m[2]),
      stat: m[3],
      etime: m[4],
      args: m[5],
    });
  }
  return procs;
}

export function listProcs(): Proc[] {
  return parsePs(
    execFileSync("ps", ["-eo", "pid=,ppid=,stat=,etime=,args="], { encoding: "utf8" }),
  );
}

/// Live (non-zombie) processes whose command line matches `pattern`, never
/// the caller itself.
export function matching(
  procs: Proc[],
  pattern: RegExp,
  exclude: number[] = [process.pid],
): Proc[] {
  return procs.filter(
    (p) => !p.stat.startsWith("Z") && !exclude.includes(p.pid) && pattern.test(p.args),
  );
}

export function isAlive(pid: number): boolean {
  return listProcs().some((p) => p.pid === pid && !p.stat.startsWith("Z"));
}

/// SIGTERM every match, wait up to `graceMs`, SIGKILL what is left. Returns
/// what was signalled (and with what) so callers can log leftovers.
export async function reap(pattern: RegExp, graceMs = 3000): Promise<Killed[]> {
  const targets = matching(listProcs(), pattern);
  const killed: Killed[] = targets.map((p) => ({ ...p, signal: "SIGTERM" }));
  for (const p of targets) signal(p.pid, "SIGTERM");
  const deadline = Date.now() + graceMs;
  let alive = targets;
  while (alive.length > 0 && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 100));
    const live = new Set(listProcs().filter((p) => !p.stat.startsWith("Z")).map((p) => p.pid));
    alive = alive.filter((p) => live.has(p.pid));
  }
  for (const p of alive) {
    signal(p.pid, "SIGKILL");
    const k = killed.find((x) => x.pid === p.pid);
    if (k) k.signal = "SIGKILL";
  }
  return killed;
}

function signal(pid: number, sig: NodeJS.Signals): void {
  try {
    process.kill(pid, sig);
  } catch {
    // already gone
  }
}

/// HOME of a process (Linux only, best effort): tells which e2e session an
/// app instance belongs to — every session runs with its own temp HOME.
export function homeOf(pid: number): string | undefined {
  try {
    const env = fs.readFileSync(`/proc/${pid}/environ`, "utf8");
    return env
      .split("\0")
      .find((kv) => kv.startsWith("HOME="))
      ?.slice("HOME=".length);
  } catch {
    return undefined;
  }
}

/// One line per process for failure messages and the CI log.
export function describeProcs(procs: Proc[]): string {
  if (procs.length === 0) return "(none)";
  const ownHome = process.env.FB_E2E_HOME;
  return procs
    .map((p) => {
      const home = homeOf(p.pid);
      const owner =
        home === undefined ? "" : home === ownHome ? " [this session]" : ` [HOME=${home}]`;
      return `pid=${p.pid} ppid=${p.ppid} stat=${p.stat} etime=${p.etime}${owner} ${p.args}`;
    })
    .join("; ");
}

/// All-thread backtraces of a live process (Linux CI, best effort): a wedged
/// webview is otherwise a black box. Needs gdb and passwordless sudo (Ubuntu's
/// ptrace_scope forbids attaching to a non-child); returns why not otherwise.
export function threadStacks(pid: number): string {
  try {
    return execFileSync(
      "sudo",
      ["-n", "gdb", "-p", String(pid), "-batch", "-ex", "thread apply all bt 30"],
      { encoding: "utf8", timeout: 120_000, stdio: ["ignore", "pipe", "pipe"] },
    );
  } catch (e) {
    return `no stacks for pid ${pid}: ${String(e).split("\n")[0]}`;
  }
}
