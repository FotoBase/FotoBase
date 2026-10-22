// TASK-01-026: report core of the perf acceptance harness.
//
// Normative source: docs/webkitgtk-perf-protocol.md (task 00-012). This
// module owns everything that must be exactly reproducible across runs and
// OSes: the §2 acceptance bars, the §3 `perf-report.json` shape, schema
// validation (what the CI smoke job asserts), and the task's meta
// self-check (inject an artificial 5 s delay ⇒ report the miss).
//
// Pure and DOM-free on purpose: the integration layer (harness.ts) collects
// the numbers; this module judges them.

/** §2 bars (exact thresholds; fps floors, RAM ceils, first-thumb strict <). */
export const BAR_FIRST_THUMB_S = 2;
export const BAR_FPS = 60;
export const BAR_IDLE_RAM_MB = 150;

/** §3 metrics. `peak_ram_mb` is a harness extension (epic wording: idle/peak RAM). */
export interface Metrics {
  time_to_first_thumbnail_s: number;
  full_scan_duration_s: number;
  scroll_fps_median: number;
  idle_ram_mb: number;
  ui_jank_events: number;
  peak_ram_mb?: number;
}

export interface Fixture {
  count: number;
  seed: number;
}

export interface Runner {
  os: string;
  webkitgtk: string;
  cpu: string;
  ram_gb: number;
}

export type Renderer = "dom" | "canvas";

export interface PerfReport {
  protocol: 1;
  fixture: Fixture;
  runner: Runner;
  renderer: Renderer;
  metrics: Metrics;
  passed: boolean;
}

/** One human-readable line per missed §2 bar. */
export function evaluateBars(m: Metrics): string[] {
  const failures: string[] = [];
  // Strict < 2 s (protocol: "bars ≥/≤ exact thresholds" — the bar itself misses).
  if (!(m.time_to_first_thumbnail_s < BAR_FIRST_THUMB_S)) {
    failures.push(
      `time_to_first_thumbnail_s ${m.time_to_first_thumbnail_s} >= ${BAR_FIRST_THUMB_S} s bar`,
    );
  }
  // fps rounded down before the >= 60 bar.
  if (Math.floor(m.scroll_fps_median) < BAR_FPS) {
    failures.push(`scroll_fps_median ${m.scroll_fps_median} < ${BAR_FPS} fps bar`);
  }
  // RAM rounded up before the < 150 MB bar.
  if (Math.ceil(m.idle_ram_mb) >= BAR_IDLE_RAM_MB) {
    failures.push(`idle_ram_mb ${m.idle_ram_mb} >= ${BAR_IDLE_RAM_MB} MB bar`);
  }
  if (m.ui_jank_events !== 0) {
    failures.push(`ui_jank_events ${m.ui_jank_events} != 0`);
  }
  return failures;
}

/** Build a §3 report; `passed` is always recomputed from the §2 bars. */
export function buildReport(input: {
  fixture: Fixture;
  runner: Runner;
  renderer: Renderer;
  metrics: Metrics;
}): PerfReport {
  return {
    protocol: 1,
    fixture: input.fixture,
    runner: input.runner,
    renderer: input.renderer,
    metrics: input.metrics,
    passed: evaluateBars(input.metrics).length === 0,
  };
}

/** Schema-validate a §3 report (what `--smoke` asserts in CI). Loose on
 * purpose about extra keys inside `runner` (environment notes), strict on
 * the protocol-critical fields. Returns one error string per violation. */
export function validateReport(doc: unknown): string[] {
  const errors: string[] = [];
  if (typeof doc !== "object" || doc === null) return ["report is not an object"];
  const r = doc as Record<string, unknown>;

  if (r.protocol !== 1) errors.push(`protocol must be 1, got ${JSON.stringify(r.protocol)}`);
  if (!isObject(r.fixture)) errors.push("fixture must be an object");
  else {
    if (typeof (r.fixture as Record<string, unknown>).count !== "number")
      errors.push("fixture.count must be a number");
    if (typeof (r.fixture as Record<string, unknown>).seed !== "number")
      errors.push("fixture.seed must be a number");
  }
  if (!isObject(r.runner)) errors.push("runner must be an object");
  else {
    const runner = r.runner as Record<string, unknown>;
    for (const key of ["os", "webkitgtk", "cpu"] as const) {
      if (typeof runner[key] !== "string") errors.push(`runner.${key} must be a string`);
    }
    if (typeof runner.ram_gb !== "number") errors.push("runner.ram_gb must be a number");
  }
  if (r.renderer !== "dom" && r.renderer !== "canvas")
    errors.push(`renderer must be "dom"|"canvas", got ${JSON.stringify(r.renderer)}`);
  if (!isObject(r.metrics)) errors.push("metrics must be an object");
  else {
    const m = r.metrics as Record<string, unknown>;
    for (const key of [
      "time_to_first_thumbnail_s",
      "full_scan_duration_s",
      "scroll_fps_median",
      "idle_ram_mb",
    ] as const) {
      if (typeof m[key] !== "number") errors.push(`metrics.${key} must be a number`);
    }
    if (typeof m.ui_jank_events !== "number") errors.push("metrics.ui_jank_events must be a number");
    if (m.peak_ram_mb !== undefined && typeof m.peak_ram_mb !== "number")
      errors.push("metrics.peak_ram_mb must be a number");
  }
  if (typeof r.passed !== "boolean") errors.push("passed must be a boolean");
  return errors;
}

/**
 * Task testing criteria (meta self-check): a synthetic run whose
 * time-to-first-thumb carries an injected +5 s delay must be reported as a
 * miss. `injectDelayS` defaults to 5; 0 yields the same synthetic run
 * without the delay (which clears every bar — the delay is provably the
 * only reason the self-check misses).
 */
export function selfCheckMetrics(opts?: { injectDelayS?: number }): Metrics {
  const inject = opts?.injectDelayS ?? 5;
  // Synthetic baseline: clears every bar with margin, as a healthy local run would.
  const baseline: Metrics = {
    time_to_first_thumbnail_s: 1.5,
    full_scan_duration_s: 60,
    scroll_fps_median: 61.4,
    idle_ram_mb: 132.2,
    ui_jank_events: 0,
  };
  return { ...baseline, time_to_first_thumbnail_s: baseline.time_to_first_thumbnail_s + inject };
}

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}
