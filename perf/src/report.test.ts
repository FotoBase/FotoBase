// TASK-01-026 tests (TDD): the report core of the perf harness.
//
// Normative source: docs/webkitgtk-perf-protocol.md (task 00-012) — the bars
// in §2, the `perf-report.json` shape in §3, and the meta self-check in the
// task's Testing criteria: inject an artificial 5 s delay ⇒ the harness
// reports the miss and exits non-zero.

import { describe, expect, it } from "vitest";

import {
  BAR_FIRST_THUMB_S,
  BAR_FPS,
  BAR_IDLE_RAM_MB,
  buildReport,
  evaluateBars,
  selfCheckMetrics,
  validateReport,
  type Metrics,
} from "./report.js";

/** Metrics that clear every §2 bar with margin. */
function passingMetrics(): Metrics {
  return {
    time_to_first_thumbnail_s: 1.2,
    full_scan_duration_s: 42,
    scroll_fps_median: 61.4,
    idle_ram_mb: 132.2,
    ui_jank_events: 0,
  };
}

describe("evaluateBars (protocol §2)", () => {
  it("passes metrics that clear every bar", () => {
    const failures = evaluateBars(passingMetrics());
    expect(failures).toEqual([]);
  });

  it("time_to_first_thumbnail_s is a strict < 2 s bar", () => {
    expect(evaluateBars({ ...passingMetrics(), time_to_first_thumbnail_s: 1.999 })).toEqual([]);
    expect(evaluateBars({ ...passingMetrics(), time_to_first_thumbnail_s: 2 })).toContainEqual(
      expect.stringContaining("time_to_first_thumbnail"),
    );
  });

  it("scroll_fps_median is floored before the >= 60 fps bar", () => {
    // 59.9 fps floors to 59 → miss; 60.9 floors to 60 → pass.
    expect(evaluateBars({ ...passingMetrics(), scroll_fps_median: 59.9 })).not.toEqual([]);
    expect(evaluateBars({ ...passingMetrics(), scroll_fps_median: 60.9 })).toEqual([]);
  });

  it("idle_ram_mb is ceiled before the < 150 MB bar", () => {
    // 149.4 MB ceils to 150 → miss; 148.4 ceils to 149 → pass.
    expect(evaluateBars({ ...passingMetrics(), idle_ram_mb: 149.4 })).not.toEqual([]);
    expect(evaluateBars({ ...passingMetrics(), idle_ram_mb: 148.4 })).toEqual([]);
  });

  it("ui_jank_events must be 0", () => {
    expect(evaluateBars({ ...passingMetrics(), ui_jank_events: 1 })).not.toEqual([]);
  });

  it("full_scan_duration_s is reported, no hard bar", () => {
    expect(evaluateBars({ ...passingMetrics(), full_scan_duration_s: 100000 })).toEqual([]);
  });

  it("every failure names the missed metric", () => {
    const failures = evaluateBars({
      time_to_first_thumbnail_s: 5,
      full_scan_duration_s: 1,
      scroll_fps_median: 30,
      idle_ram_mb: 999,
      ui_jank_events: 3,
    });
    expect(failures).toHaveLength(4);
    for (const key of ["time_to_first_thumbnail", "scroll_fps", "idle_ram", "ui_jank"]) {
      expect(failures.some((f) => f.includes(key))).toBe(true);
    }
  });
});

describe("buildReport (protocol §3)", () => {
  it("emits the protocol-1 shape with computed `passed`", () => {
    const report = buildReport({
      fixture: { count: 1000, seed: 42 },
      runner: { os: "ubuntu-22.04", webkitgtk: "2.48.0", cpu: "test", ram_gb: 8 },
      renderer: "dom",
      metrics: passingMetrics(),
    });
    expect(report.protocol).toBe(1);
    expect(report.fixture).toEqual({ count: 1000, seed: 42 });
    expect(report.renderer).toBe("dom");
    expect(report.passed).toBe(true);
    expect(report.metrics).toEqual(passingMetrics());
  });

  it("`passed` is computed from the bars, never trusted from input", () => {
    const report = buildReport({
      fixture: { count: 50000, seed: 42 },
      runner: { os: "ubuntu-22.04", webkitgtk: "2.48.0", cpu: "test", ram_gb: 8 },
      renderer: "canvas",
      // Deliberately lying `passed: true` must be recomputed to false.
      metrics: { ...passingMetrics(), time_to_first_thumbnail_s: 5 },
    });
    expect(report.passed).toBe(false);
  });
});

describe("validateReport (CI smoke asserts schema validity)", () => {
  it("accepts a well-formed report", () => {
    const report = buildReport({
      fixture: { count: 1000, seed: 42 },
      runner: { os: "ubuntu-22.04", webkitgtk: "2.48.0", cpu: "x86_64", ram_gb: 16 },
      renderer: "dom",
      metrics: passingMetrics(),
    });
    expect(validateReport(report)).toEqual([]);
  });

  it("accepts the optional peak_ram_mb extension", () => {
    const report = buildReport({
      fixture: { count: 1000, seed: 42 },
      runner: { os: "ubuntu-22.04", webkitgtk: "2.48.0", cpu: "x86_64", ram_gb: 16 },
      renderer: "dom",
      metrics: { ...passingMetrics(), peak_ram_mb: 180 },
    });
    expect(validateReport(report)).toEqual([]);
  });

  it("rejects a wrong protocol version", () => {
    expect(validateReport({ ...validReport(), protocol: 2 })).not.toEqual([]);
  });

  it("rejects a missing metric and a wrong metric type", () => {
    const missing = validReport();
    delete (missing.metrics as Record<string, unknown>).idle_ram_mb;
    expect(validateReport(missing)).not.toEqual([]);

    expect(
      validateReport({ ...validReport(), metrics: { ...passingMetrics(), ui_jank_events: "0" } }),
    ).not.toEqual([]);
  });

  it("rejects an unknown renderer and a non-boolean passed", () => {
    expect(validateReport({ ...validReport(), renderer: "webgl" })).not.toEqual([]);
    expect(validateReport({ ...validReport(), passed: "yes" })).not.toEqual([]);
  });

  it("rejects a fixture/runner shape violation", () => {
    expect(validateReport({ ...validReport(), fixture: { count: 1000 } })).not.toEqual([]);
    expect(validateReport({ ...validReport(), runner: { os: "ubuntu-22.04" } })).not.toEqual([]);
  });

  function validReport(): Record<string, unknown> {
    return buildReport({
      fixture: { count: 1000, seed: 42 },
      runner: { os: "ubuntu-22.04", webkitgtk: "2.48.0", cpu: "x86_64", ram_gb: 16 },
      renderer: "dom",
      metrics: passingMetrics(),
    }) as unknown as Record<string, unknown>;
  }
});

describe("self-check (task testing criteria: injected 5 s delay ⇒ miss)", () => {
  it("selfCheckMetrics delays first-thumb past the bar", () => {
    const metrics = selfCheckMetrics();
    // 1.5 s baseline + 5 s injected delay = 6.5 s: at the bar itself, a miss.
    expect(metrics.time_to_first_thumbnail_s).toBeGreaterThanOrEqual(BAR_FIRST_THUMB_S);
    expect(evaluateBars(metrics)).not.toEqual([]);
  });

  it("the injected delay is the only reason the self-check misses", () => {
    // Without the +5 s the same synthetic run clears every bar.
    const fast = selfCheckMetrics({ injectDelayS: 0 });
    expect(evaluateBars(fast)).toEqual([]);
    const delayed = selfCheckMetrics();
    expect(delayed.time_to_first_thumbnail_s - fast.time_to_first_thumbnail_s).toBe(5);
  });

  it("exposes the bars it enforces", () => {
    expect(BAR_FIRST_THUMB_S).toBe(2);
    expect(BAR_FPS).toBe(60);
    expect(BAR_IDLE_RAM_MB).toBe(150);
  });
});
