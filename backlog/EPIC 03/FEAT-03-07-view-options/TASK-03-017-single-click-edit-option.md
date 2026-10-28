# task: single-click-to-edit option

**Feature:** [FEAT-03-07-view-options](FEAT-03-07-view-options.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in review (2026-10-28)

## Description
General option: single click opens the edit view instead of requiring double-click (PLAN §6 P2a "single-click-to-edit option"). Small Picasa-parity preference.

## Functional specs
- Option in General options, default off (single click selects, as today).
- When on, single-clicking a thumbnail opens the edit/viewer for that photo; selection still possible via modifier-click or the option being off.
- Choice persists across restarts.

## Technical specs
- Frontend flag in the Options store; grid click handler branches on it.
- No backend involvement; edit view entry reuses the existing double-click route.

## Testing criteria (TDD)
- E2e: with option off, single click selects only; with option on, single click opens the edit view; restart → persisted.

## Acceptance criteria
- [x] 03-017: single-click-to-edit option in General options, default off.
- [x] 03-017: behavior switches correctly and persists.
