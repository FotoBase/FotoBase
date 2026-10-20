# task: folder sort and flat vs tree view

**Feature:** [FEAT-03-07-view-options](FEAT-03-07-view-options.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** todo

## Description
Folder sorting (name/date/size, with reverse) and a flat vs tree folder view, working over the full library (PLAN §6 P2a). Pure view-layer, no catalog changes.

## Functional specs
- Sort folders by name, date, or size; reverse toggle; choice persists.
- Flat view lists all folders as a single sortable list; tree view keeps the P1 year-grouped tree.
- Both views perform over the full library without jank.

## Technical specs
- Frontend-only: sort over the folder list already loaded; size/date aggregate from `folders`/`files` counts already in the store.
- Persist sort + view mode in Options; default stays tree/name (P1 behavior).

## Testing criteria (TDD)
- Unit: sort comparator for the three keys + reverse on a fixture folder list.
- E2e: switch flat/tree, cycle sorts, restart → persisted.

## Acceptance criteria
- [ ] 03-016: folders sortable by name/date/size with reverse; persists.
- [ ] 03-016: flat and tree views both work over the full library without jank.
