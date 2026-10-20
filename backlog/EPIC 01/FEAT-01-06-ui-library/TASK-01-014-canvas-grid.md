# task: build canvas-grid renderer fallback

**Feature:** [FEAT-01-06-ui-library](FEAT-01-06-ui-library.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** todo

## Description
Implement the canvas-grid renderer (PLAN §6 P1: canvas grid is a planned deliverable, not a contingency): draws thumbnails to a canvas instead of DOM nodes. Linux default if the 01-004 ADR says so; available as fallback on every OS.

## Functional specs
- Same UX as the DOM grid: zoom slider, selection, context-menu coordinates, scroll performance.
- Switchable (config/ADR-driven default); user can override.
- 60 fps on the 50k fixture on WebKitGTK when it is the chosen default (§6 acceptance).

## Technical specs
- Single `<canvas>` with offscreen-decoded thumbnails drawn per visible cell; reuses `thumb://` cache bytes (fetched into the render layer, still no IPC payloads).
- Shares grid state (scroll position, selection) with the DOM implementation via a common store so both stay in sync feature-wise.

## Testing criteria (TDD)
- E2E: canvas grid renders the 1k fixture; zoom slider and selection work; fps harness run on Linux passes the §6 bar with the chosen default.

## Acceptance criteria
- [ ] 01-014: Canvas-grid renderer with zoom/selection parity; Linux default per the 01-004 ADR; available as fallback everywhere.
