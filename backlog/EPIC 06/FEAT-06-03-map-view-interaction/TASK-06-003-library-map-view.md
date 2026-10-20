# task: build library-wide map view

**Feature:** [FEAT-06-03-map-view-interaction](FEAT-06-03-map-view-interaction.md) · **Epic:** [EPIC-06 — Places](../EPIC-06.md) · **Status:** todo

## Description
View → Map: a library-wide world map with a pin (or cluster) for every geotagged photo in the catalog; the current search/filter state applies to the map live (PLAN §2 View menu "Map"; §6 P3 Places).

## Functional specs
- Every geotagged photo in the catalog appears as a pin (clustered per 06-002).
- Current search/filters apply live: changing the search updates the map's pins.
- Works with mouse and touch.

## Technical specs
- Svelte map view over MapLibre (06-001 infrastructure); pin data = GPS query over the catalog filtered by the active search/filter state (FTS + filters from EPIC-03).
- Live updates: re-query on filter/search change, debounced.

## Testing criteria (TDD)
- E2e: open map with a filtered search → only matching geotagged photos pinned; change search → pins update.
- Integration: pin set equals the filtered GPS query result on the fixture.

## Acceptance criteria
- [ ] 06-003: Library-wide map shows a pin per geotagged photo; search/filters apply live.
