# task: build perf acceptance harness

**Feature:** [FEAT-01-05-perf-acceptance-harness](FEAT-01-05-perf-acceptance-harness.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** todo

## Description

Build the perf harness on the 00-013 fixture (PLAN §9 Performance row, §10 WebView-drift risk): measures time-to-first-thumbnail, idle/peak RAM, and scroll fps per OS, emits machine-readable results, and runs as a CI smoke test that fails loudly when the §6 P1 bars are missed.

## Functional specs

- Measures: time-to-first-thumb (< 2 s bar), idle RAM (< 150 MB bar), scroll fps (60 fps bar) — per OS.
- CI smoke mode fails the build when a bar is missed.
- Results comparable across runs: same fixture seed, same metrics, stable environment notes recorded.

## Technical specs

- CI smoke wired into `.github/workflows/perf-smoke.yml` (1k fixture on ubuntu-latest; asserts schema-valid `perf-report.json`, per the 00-012 protocol — full 50k decision runs stay manual/local).
- Driven by the 00-012 protocol's metric definitions; consumes the 00-013 fixture one-liner (1k in CI, 50k for full runs).
- E2E via WebdriverIO + tauri-driver (§4); fps via rAF timing in the page; RAM from OS process metrics.
- Output: JSON results file committed per release for trend comparison.

## Testing criteria (TDD)

- Meta: harness self-check — inject an artificial 5 s delay ⇒ harness reports the miss and exits non-zero.

## Acceptance criteria

- [ ] 01-026: Harness measures first-thumb/RAM/fps per OS on the 00-013 fixture, runs as CI smoke, fails on missed §6 bars, emits comparable results.
