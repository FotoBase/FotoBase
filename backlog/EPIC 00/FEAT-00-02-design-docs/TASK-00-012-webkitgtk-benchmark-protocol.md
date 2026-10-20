# task: define WebKitGTK perf benchmark protocol

**Feature:** [FEAT-00-02-design-docs](FEAT-00-02-design-docs.md) · **Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** done (2026-10-20) → [`docs/webkitgtk-perf-protocol.md`](../../../docs/webkitgtk-perf-protocol.md)

## Description
Write the WebKitGTK benchmark protocol (PLAN §10 risk 1): which fixture to use (the 00-013 50k generator), which metrics to measure, and the numeric perf bar that triggers the canvas grid on Linux. Runs as the EPIC-01 entry gate.

## Functional specs
- Protocol is runnable end-to-end on ubuntu-latest (CI).
- Defines metrics: time-to-first-thumbnail, scroll fps, idle RAM (matching §6 P1 acceptance).
- States the numeric bar; missing it ⇒ canvas grid becomes the Linux default.

## Technical specs
- Uses the 00-013 fixture (deterministic seed, configurable size).
- Emits machine-readable results so 01-004 (decision ADR) and 01-026 (CI harness) consume the same output.

## Testing criteria (TDD)
- CI (protocol dry-run): runs on ubuntu-latest and produces a results file even on a small fixture.

## Acceptance criteria
- [x] 00-012: Protocol doc in `docs/` defining fixture, metrics, and the canvas-trigger perf bar.
- [ ] Runnable on ubuntu-latest; EPIC-01 entry gate satisfied.
