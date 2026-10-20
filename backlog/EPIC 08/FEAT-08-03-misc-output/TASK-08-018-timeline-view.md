# task: add full-screen timeline view

**Feature:** [FEAT-08-03-misc-output](FEAT-08-03-misc-output.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
Full-screen chronological browsing of folders/photos by date (PLAN §2 §B Timeline P4, View menu "Timeline: full-screen chronological browsing").

## Functional specs
- View → Timeline opens a full-screen view; photos/folders appear on a date-scaled horizontal timeline, grouped by date.
- Scrolling/scrubbing moves through time smoothly; clicking a group opens the folder/grid at that date.
- Respects the current search/filters (consistent with the rest of the app).
- Keyboard navigable (arrow keys scrub) per PLAN §2 §M keyboard control.

## Technical specs
- UI (Svelte): one new view route; date axis + thumbnail clusters rendered on a canvas (reuse the P1 canvas-grid machinery — a DOM timeline of 50k items won't fly).
- Data: grouped query over `files`/`folders` by `taken_at`; incremental fetch per visible date window.
- No new storage; pure view over the catalog.

## Testing criteria (TDD)
- Integration: fixture spanning 3 years → groups appear at correct date positions in order.
- E2E: open timeline, scrub with keyboard, click a group → lands in the grid at that date.
- Performance: 50k-fixture timeline scrub at 60 fps (perf harness, PLAN §9).

## Acceptance criteria
- [ ] 08-018: Timeline view offers full-screen chronological browsing, keyboard-scrubbable, respecting active filters.
- [ ] 08-018: Smooth on the 50k fixture (no jank while scrubbing).
