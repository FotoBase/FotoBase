# task: add geotagged filter

**Feature:** [FEAT-06-01-geotagging](FEAT-06-01-geotagging.md) · **Epic:** [EPIC-06 — Places](../EPIC-06.md) · **Status:** todo

## Description
"Geotagged" filter button showing only photos with GPS, in both grid and map views (PLAN §2C filter buttons; §6 P3 Places).

## Functional specs
- Filter toggle restricts the grid to geotagged photos; combined with other filters/search.
- In map view the filter state is already implicit; the button lives with the other filter buttons (starred, videos, faces).

## Technical specs
- SQL filter over non-null GPS in catalog `metadata`/`files`; consistent with the filter-button framework from EPIC-03 (§2C).

## Testing criteria (TDD)
- Integration: mixed fixture → filter returns exactly the photos with GPS set.
- E2e: toggle filter in grid; combined with a search term.

## Acceptance criteria
- [ ] 06-007: "Geotagged" filter works in grid and map contexts.
