# task: build virtualized DOM thumbnail grid with zoom slider

**Feature:** [FEAT-01-06-ui-library](FEAT-01-06-ui-library.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in review (2026-10-21)

## Description
Implement the DOM virtualized thumbnail grid per PLAN §4 (TanStack Virtual): renders only visible thumbnails, handles 100k+ items, with a live zoom slider (§2A "Zoomable grid with size slider").

## Functional specs
- Grid scrolls at 60 fps on the 50k fixture (§6 P1 acceptance) — DOM implementation is the default; canvas fallback per 01-014 where 01-004 dictates.
- Zoom slider resizes thumbnails live without re-fetching (CSS size; re-render at new size via cache when needed).
- Thumbnails load via `thumb://` (01-011); scrolling reprioritizes render jobs (01-009).

## Technical specs
- Svelte 5 + TanStack Virtual; plain CSS grid; image srcs are `thumb://` URIs only (no IPC bytes).
- Visible-range events feed the job queue's priority for visible-first rendering.

## Testing criteria (TDD)
- E2E: on the 1k fixture, scroll fast ⇒ only visible rows have mounted DOM nodes (node count stays bounded).
- Perf: 50k-fixture scroll fps ≥ 60 on the harness (01-026) per OS.

## Acceptance criteria
- [x] 01-013: Virtualized DOM grid (TanStack Virtual) with live zoom slider; bounded DOM; 60 fps target.
