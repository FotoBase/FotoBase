# task: filter buttons and date slider

**Feature:** [FEAT-03-05-search-filters](FEAT-03-05-search-filters.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** todo

## Description
Filter buttons (starred, video) and a date slider restricting the library to the last N days/months (PLAN §2 C "Filter buttons" + "Date slider", §6 P2a). Combines with live search; grid updates without jank.

## Functional specs
- Toggle buttons for starred-only and videos-only; date slider restricts to last N days/months.
- Filters AND-combine with each other and with the search text; clearing is one click each.
- Filters apply consistently in grid and viewer contexts; state persists for the session.

## Technical specs
- Filters compile into the existing query pipeline (SQL WHERE over `files` + FTS); starred = `xmp:Rating` ≥ threshold, video = `files.media_type`.
- Date slider maps slider position → `taken_at >= cutoff` (derived data, catalog-side — no sidecar involvement).
- Frontend: debounced slider; grid re-query through existing virtualized grid path.

## Testing criteria (TDD)
- Unit: query builder — starred+video+date-cutoff fixture returns exactly the expected photo set.
- E2e: toggling each filter and combining with a search term updates the grid correctly; behavior identical in viewer.
- Perf: filter toggle on 50k fixture re-renders without jank (frame budget check).

## Acceptance criteria
- [ ] 03-014: starred and video filter buttons work.
- [ ] 03-014: date slider restricts to last N days/months.
- [ ] 03-014: filters combine with search and apply in grid and viewer.
