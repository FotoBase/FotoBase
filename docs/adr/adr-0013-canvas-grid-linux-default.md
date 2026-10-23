# ADR-0013: Canvas grid is the Linux default

**Date:** 2026-10-23 · **Status:** accepted

## Context

Task 00-012 defined the WebKitGTK perf protocol (`docs/webkitgtk-perf-protocol.md`); task 01-004 requires it be run and the DOM-vs-canvas decision recorded with numbers (PLAN §10 risk 1, §6 P1 acceptance). Two full 50k protocol runs (seed 42, cold start, empty catalog + empty thumbnail cache, `perf/Dockerfile.decision` container) on the decision hardware:

| Metric | Bar | Run 1 | Run 2 |
|---|---|---|---|
| `time_to_first_thumbnail_s` | < 2 s | 3.5 | 3.4 |
| `scroll_fps_median` | ≥ 60 fps | 44.2 | 47.7 |
| `idle_ram_mb` | < 150 MB | 151.7 | 150.6 |
| `ui_jank_events` | 0 | 120 | 115 |

Runner: Fedora 44 (kernel 7.2.11), WebKitGTK 2.50.4, AMD Ryzen 7 9800X3D, 60 GB RAM. Reports: `perf/results/run1-2026-10-23.json`, `perf/results/run2-2026-10-23.json`. Protocol §4 rule 3: any bar missed → canvas grid becomes the Linux default from day one.

## Decision

**The canvas grid is the Linux default renderer from day one.** The DOM grid ships as an opt-in fallback setting (protocol §2 decision rule) and remains the default on Windows (WebView2) and macOS (WKWebView) unless per-release perf runs there show regressions.

Justification:

- **scroll fps is decisive.** 44.2 / 47.7 fps against a 60 bar on the fastest plausible desktop CPU with WebKitGTK 2.50.4 — well above the 2.40 floor and far above the "old hardware" positioning target (Ubuntu 22.04, older machines). No realistic DOM tuning closes a 20–26% gap on strong hardware; it only widens on weak hardware.
- **time_to_first_thumbnail 3.4–3.5 s vs full scan 2.3 s:** ~1.2 s of post-scan thumbnail latency (visible-first priority, PLAN invariant 4), but fixing it cannot change the decision — fps alone fails.
- **idle_ram_mb 150.6–151.7 vs 150 bar:** marginal (≤1.7 MB, within run-to-run noise), but irrelevant — the decision is already settled by fps.
- **ui_jank_events 115–120:** main-thread stalls during scan + scroll; the DOM grid's emit/re-list traffic is a contributing factor. The canvas renderer sidesteps the scroll-path jank; scan-path jank is tracked separately (backfill/coalesce work, e.g. the `faaed33` throttling).
- **Reproducibility (task TDD criterion):** both runs miss the same four bars with comparable magnitudes (Δ ≤ 3.5 fps, ≤ 1.1 MB, ≤ 5 jank) — the result is stable, not noise.

## Consequences

- Canvas grid (01-014) is the primary P1 deliverable on Linux, not a contingency; it must reach all §2 bars on WebKitGTK before EPIC-01 closes.
- DOM grid stays in the codebase as the opt-in Linux setting and the default on Windows/macOS; per-release perf runs (protocol §4 rule 4) may flip defaults back if WebView drift is reversed.
- The 60 fps / < 2 s / < 150 MB bars still bind P1 acceptance on all three OSes — the canvas renderer now has to pass them, not just exist.
- DOM-grid perf work on Linux (jank during scan, TTFT) is demoted from "must pass" to best-effort, but the jank sources also affect the canvas path's data pipeline and remain worth fixing.

---

<!-- docs-lint: sections Date · Status · Context · Decision · Consequences present; linked from PLAN §6 (canvas-grid fallback bullet) and §10 (WebKitGTK risk row). -->
