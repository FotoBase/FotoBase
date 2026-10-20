# task: embed geonames dataset and reverse geocode

**Feature:** [FEAT-06-04-reverse-geocoding](FEAT-06-04-reverse-geocoding.md) · **Epic:** [EPIC-06 — Places](../EPIC-06.md) · **Status:** todo

## Description
Offline reverse geocoding from an embedded GeoNames cities dataset: GPS → place names in the catalog, searchable via FTS ("photos in Lisbon") (PLAN §4 "GeoNames cities dataset (embedded)"; §6 P3 Places).

## Functional specs
- Fully offline: coordinates resolve to city-level place names.
- Place names land in the catalog `places` table and FTS index; live search finds photos by place.
- Bulk geocoding of the 50k fixture runs as a background job without UI jank.

## Technical specs
- Embedded GeoNames cities dataset (bundled or opt-in download consistent with size goals); nearest-city lookup with distance threshold for "city-level accuracy".
- Results in catalog `places` (geo cell + place name) and `files_fts` place column (§5) — derived data, recomputable on rebuild.
- Reverse geocode job in the job queue at background priority, run after GPS changes.

## Testing criteria (TDD)
- Unit: known coordinates (e.g. Lisbon, Tokyo) → expected city names.
- Integration: geotag a photo → place name appears; FTS search "Lisbon" returns it.
- Integration: bulk geocode of 50k fixture completes as background job, UI responsive (§9 Performance).

## Acceptance criteria
- [ ] 06-008: Embedded GeoNames dataset powers offline reverse geocoding to place names.
- [ ] 06-008: Place names in catalog + FTS; bulk geocoding of 50k fixture is a non-blocking background job.
