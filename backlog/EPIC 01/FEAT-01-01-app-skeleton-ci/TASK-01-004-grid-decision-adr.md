# task: run webkitgtk benchmark and decide dom vs canvas

**Feature:** [FEAT-01-01-app-skeleton-ci](FEAT-01-01-app-skeleton-ci.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in progress (2026-10-22)

## Description

Execute the 00-012 benchmark protocol on WebKitGTK and record the DOM-vs-canvas grid decision in an ADR with numeric justification (PLAN §10 risk 1: WebKitGTK is the weakest WebView; canvas grid is a planned deliverable, not a contingency).

## Functional specs

- Benchmark runs on the 00-013 fixture per the 00-012 protocol on ubuntu-latest.
- ADR states the Linux default (DOM or canvas) with the measured numbers.

## Technical specs

- Metrics per §6 P1 acceptance: time-to-first-thumbnail, scroll fps, idle RAM.
- Result file feeds the ADR; the decision sets the default for 01-014.

## Testing criteria (TDD)

- Reproducibility: two protocol runs on the same seed produce comparable numbers (within stated tolerance).

## Acceptance criteria

- [x] 01-004: Benchmark run recorded; ADR in `docs/adr/` decides DOM vs canvas as Linux default with numeric justification.
