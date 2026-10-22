# task: wire live search bar to FTS

**Feature:** [FEAT-01-06-ui-library](FEAT-01-06-ui-library.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in review (2026-10-22)

## Description
Implement the live search bar (PLAN §2C: "Live search bar filters while typing"), wired to the 01-008 FTS API over filenames and folder names. P1 scope is files/folders; metadata search arrives in P2.

## Functional specs
- Typing filters the grid live across filenames and folder names, with no UI jank on the 50k fixture.
- Clearing the search restores the previous folder selection/filter state.
- Search combines with folder-tree selection (both filters apply).

## Technical specs
- Debounced queries (tens of ms) against the FTS query API; results are file ids rendered by the existing grid.
- Query state lives in the shared grid store so DOM and canvas grids get filtering for free.

## Testing criteria (TDD)
- E2E: type a filename fragment on the 1k fixture ⇒ grid narrows to matches while typing; clear ⇒ full grid returns.
- E2E: folder selected + search term ⇒ intersection shown.

## Acceptance criteria
- [x] 01-018: Live search bar wired to FTS (filenames/folders); combines with folder selection; no jank on 50k.
