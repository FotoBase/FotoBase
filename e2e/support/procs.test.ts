// Unit tests for the harness's process helpers (TASK-05-013). Run with
// `npm run test:unit` — plain node:test, no driver or app needed, so they
// also run on macOS.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { test } from "node:test";

import { APP, isAlive, listProcs, matching, parsePs, reap } from "./procs";

test("parsePs keeps args with spaces intact", () => {
  const out = [
    "  101     1 Ss   01:02:03 /usr/bin/tauri-driver --port 4444",
    "  202   101 Sl      00:05 /home/runner/work/FotoBase/FotoBase/target/debug/fotobase --flag x",
    "",
  ].join("\n");
  assert.deepEqual(parsePs(out), [
    { pid: 101, ppid: 1, stat: "Ss", etime: "01:02:03", args: "/usr/bin/tauri-driver --port 4444" },
    {
      pid: 202,
      ppid: 101,
      stat: "Sl",
      etime: "00:05",
      args: "/home/runner/work/FotoBase/FotoBase/target/debug/fotobase --flag x",
    },
  ]);
});

test("APP matches the app binary only, never a shell quoting the pattern", () => {
  const procs = parsePs(
    [
      "1 0 S 00:01 /repo/target/debug/fotobase",
      "2 0 S 00:01 /repo/target/debug/fotobase --x",
      "3 0 Z 00:01 [fotobase] <defunct>",
      "4 0 S 00:01 /bin/sh -c pkill -f '/fotobase( |$)' || true",
      "5 0 S 00:01 node /repo/e2e/node_modules/fotobase-e2e/run.js",
    ].join("\n"),
  );
  assert.deepEqual(
    matching(procs, APP, []).map((p) => p.pid),
    [1, 2],
  );
});

test("matching never returns the caller's own pid", () => {
  const procs = parsePs(`${process.pid} 1 S 00:01 /repo/target/debug/fotobase`);
  assert.deepEqual(matching(procs, APP), []);
});

// The regression the task is about: `execSync("pkill -f tauri-driver || …")`
// matched — and killed — its own `sh -c` on Linux, so the rest of the cleanup
// never ran. reap() runs without a shell and survives its own pattern.
test("reap kills matches, escalates to SIGKILL, and survives its own pattern", async () => {
  const marker = `fb-e2e-reap-${process.pid}-${Date.now()}`;
  const polite = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)", marker]);
  const stubborn = spawn(process.execPath, [
    "-e",
    "process.on('SIGTERM', () => {}); setInterval(() => {}, 1000)",
    marker,
  ]);
  try {
    // Let both start (and the stubborn one install its handler).
    await new Promise((r) => setTimeout(r, 500));
    const pattern = new RegExp(marker);
    assert.equal(matching(listProcs(), pattern).length, 2);

    const killed = await reap(pattern, 1000);

    assert.deepEqual(killed.map((k) => k.pid).sort(), [polite.pid, stubborn.pid].sort());
    assert.equal(killed.find((k) => k.pid === stubborn.pid)?.signal, "SIGKILL");
    assert.equal(killed.find((k) => k.pid === polite.pid)?.signal, "SIGTERM");
    assert.equal(isAlive(polite.pid!), false);
    assert.equal(isAlive(stubborn.pid!), false);
    assert.equal(matching(listProcs(), pattern).length, 0);
  } finally {
    polite.kill("SIGKILL");
    stubborn.kill("SIGKILL");
  }
});
