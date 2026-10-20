# feat: reverse geocoding

**Epic:** [EPIC-06 — Places](../EPIC-06.md) · **Status:** planned

## Acceptance criteria

- Reverse geocoding runs fully offline from the embedded GeoNames dataset; typical city-level accuracy on test coordinates.
- Place names land in the catalog and are searchable via FTS ("photos in Lisbon").
- Bulk geocoding of the 50k fixture completes as a background job without UI jank.

- [ ] `06-008` (M) Embedded GeoNames cities dataset; offline reverse geocode → place names in catalog + FTS.

## Tasks

- [ ] [TASK-06-008-geonames-reverse-geocode.md](TASK-06-008-geonames-reverse-geocode.md) — embed geonames dataset and reverse geocode
