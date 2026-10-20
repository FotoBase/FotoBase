---
name: perf-acceptance
description: Run the WebKitGTK perf protocol — generate fixtures, produce perf-report.json, compare against the PLAN budgets, and decide DOM-vs-canvas. Use for perf runs, perf regressions, CI smoke perf checks, or release-candidate perf validation.
---

# Perf acceptance run

Follow `docs/webkitgtk-perf-protocol.md` (task 00-012) — it is normative. Summary of the loop:

## 1. Generate fixtures

```
python3 tools/fixture_gen.py --count <N> --seed 42 --out <dir>
```

- Deterministic: same (seed, count) ⇒ identical bytes. Never commit generated data.
- CI smoke: `--count 1000`. Perf decision run: `--count 50000` (local, on recorded reference hardware).
- HEIC samples are copied from `tools/fixture-samples/`; the generator fails if they're missing.

## 2. Run the harness

- WebdriverIO + `tauri-driver`; record runner info: OS, WebKitGTK version, CPU, RAM GB.
- Metrics: `time_to_first_thumbnail_s`, `full_scan_duration_s`, `scroll_fps_median` (median of 5 runs), `idle_ram_mb`, `ui_jank_events` (stalls > 100 ms).

## 3. Emit `perf-report.json`

Schema (protocol §report): `protocol: 1`, `fixture {count, seed}`, `runner {os, webkitgtk, cpu, ram_gb}`, `renderer: "dom"|"canvas"`, `metrics {…}`, `passed` (bool; fps rounded down, RAM rounded up).

## 4. Compare against the bars

| Bar | Budget |
|---|---|
| `time_to_first_thumbnail_s` | < 2 s |
| `scroll_fps_median` | ≥ 60 fps |
| `idle_ram_mb` | < 150 MB |
| `ui_jank_events` | 0 |

`full_scan_duration_s` is reported, no hard bar. Installer size (< 20 MB) is checked once per release build, not by this harness.

## 5. Report and act

- Report pass/fail per bar with the measured numbers and cite the protocol doc.
- **CI smoke (1k):** asserts only that the report is produced and schema-valid — do not fail on bars.
- **Decision run (50k) misses any bar on the reference machine:** the canvas grid becomes the Linux default and this is recorded in an ADR (per task 01-004). Flag it; do not flip silently.
- Decision runs are re-run per release; commit the resulting JSON for trend comparison.
