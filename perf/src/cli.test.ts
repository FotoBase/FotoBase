// TASK-01-026 (TDD, red): CLI contract of the harness. The full run needs a
// Tauri app + tauri-driver (integration territory); what the task's Testing
// criteria pin down test-first is the self-check: inject an artificial 5 s
// delay ⇒ the harness reports the miss and exits non-zero.

import { describe, expect, it } from "vitest";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const run = promisify(execFile);

describe("harness CLI", () => {
  it("--self-check reports the injected miss and exits non-zero", async () => {
    let code: number | undefined;
    let stdout = "";
    try {
      const res = await run("tsx", ["src/harness.ts", "--self-check"], {
        cwd: new URL("..", import.meta.url).pathname,
        timeout: 60_000,
      });
      stdout = res.stdout;
      code = 0;
    } catch (e) {
      const err = e as { code?: number; stdout?: string };
      code = err.code;
      stdout = err.stdout ?? "";
    }
    expect(code, `expected non-zero exit, got ${code}`).not.toBe(0);
    expect(stdout).toContain('"passed": false');
    expect(stdout).toContain("time_to_first_thumbnail");
  }, 90_000);
});
