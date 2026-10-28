# task: thumbnail caption modes

**Feature:** [FEAT-03-07-view-options](FEAT-03-07-view-options.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in review (2026-10-28)

## Description
Thumbnail captions selectable per grid: none, filename, caption, tags, or resolution (PLAN §6 P2a "Thumbnail captions"). Switch is instant and persists across restarts.

## Functional specs
- Mode switcher (grid toolbar or view menu); switching re-renders instantly, no rescan.
- Choice persists across restarts (app option).
- Long values truncate with ellipsis; tags mode shows a compact tag list.

## Technical specs
- Pure frontend: caption text comes from data already in the grid's row model (filename, caption/tags from catalog, dimensions from `files`).
- Persist via the existing Options store (P1 General options).
- No layout thrash: fixed caption strip height per zoom level.

## Testing criteria (TDD)
- Unit: mode → caption-text resolver per photo (each of the 5 modes).
- E2e: switch through all modes; restart → mode persisted.

## Acceptance criteria
- [x] 03-015: all five caption modes render correctly under thumbnails.
- [x] 03-015: switching is instant and persists across restarts.
