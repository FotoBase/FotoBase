# WebKitGTK performance benchmark protocol

**Task:** 00-012 · **Status:** approved 2026-10-20 · **Gates:** EPIC-01 entry (DOM grid vs canvas-grid decision, 01-004/01-026) · **Plan refs:** §6 P1 acceptance, §10 risk 1

Defines the protocol that decides whether the DOM grid meets the P1 perf bar on WebKitGTK, or the canvas renderer becomes the Linux default. The canvas grid is a **planned deliverable either way** (§6 P1); this protocol only decides which renderer is the Linux default.

---

## 1. Environment

- **OS/WebView:** ubuntu-latest CI runner + WebKitGTK 2.40+ (the §11 Q4 floor); one local run per release on the oldest supported Ubuntu 22.04 x64 machine available (the "old hardware" positioning target).
- **Fixture:** the 00-013 generator (`fixture gen --seed 42 --out <dir>`).
  - CI smoke: `--count 1000` (fits CI time budget; catches regressions only).
  - Perf decision run: `--count 50000` (local, hardware recorded, results committed as JSON).
- Cold app start with empty catalog + empty thumbnail cache; fixture directory added as watched folder; run continues through full scan completion.

## 2. Metrics

All captured by the harness into a single machine-readable `perf-report.json` (schema below) consumed by 01-004 (decision ADR) and 01-026 (CI harness).

| Metric | Measurement | P1 acceptance bar |
|---|---|---|
| `time_to_first_thumbnail` | scan start → first grid thumbnail visible | **< 2 s** |
| `full_scan_duration` | scan start → scan-complete event (background, UI not blocked) | reported, no hard bar |
| `scroll_fps` | scripted grid scroll (fast flick, 10 s) via frame timestamps; median of 5 runs | **≥ 60 fps** |
| `idle_ram_mb` | after full scan, 30 s quiescent, app process tree RSS | **< 150 MB** |
| `ui_jank_events` | main-thread stalls > 100 ms during scan + scroll | **0** |

## 3. `perf-report.json` schema

```json
{
  "protocol": 1,
  "fixture": { "count": 50000, "seed": 42 },
  "runner": { "os": "ubuntu-22.04", "webkitgtk": "2.40.x", "cpu": "...", "ram_gb": 8 },
  "renderer": "dom" | "canvas",
  "metrics": {
    "time_to_first_thumbnail_s": 1.8,
    "full_scan_duration_s": 640,
    "scroll_fps_median": 58,
    "idle_ram_mb": 132,
    "ui_jank_events": 2
  },
  "passed": false
}
```

`passed` is computed by the harness from the bars in §2 (bars ≥/≤ exact thresholds; fps rounded down, RAM rounded up).

## 4. Decision rule

1. Run the 50k protocol with the **DOM grid** on WebKitGTK.
2. All bars pass → DOM grid stays the Linux default; canvas grid ships as the fallback setting.
3. Any bar missed on the reference machine → **canvas grid becomes the Linux default from day one** (§6 P1), recorded in the 01-004 ADR with the report attached.
4. Re-run per release (perf smoke in CI at 1k; full 50k run on release-candidate builds) — WebView drift is a listed §10 risk, so regressions flip the default the same way.

## 5. CI dry-run (protocol self-test)

- A CI job runs the harness end-to-end on ubuntu-latest with the 1k fixture and asserts only that `perf-report.json` is produced and is schema-valid — not the perf bars (CI hardware is not the decision hardware).
- This job gates EPIC-01's entry: harness must exist and be green before grid work begins.
