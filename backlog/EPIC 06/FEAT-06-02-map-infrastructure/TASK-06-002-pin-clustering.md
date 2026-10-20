# task: implement map pin clustering

**Feature:** [FEAT-06-02-map-infrastructure](FEAT-06-02-map-infrastructure.md) · **Epic:** [EPIC-06 — Places](../EPIC-06.md) · **Status:** todo

## Description
Marker clustering: pins cluster at low zoom and resolve into photo thumbnails as zoom increases, down to individual pins — smooth on a 50k-photo library (PLAN §2D Map view; §6 P3 Places epic acceptance).

## Functional specs
- Low zoom: grouped cluster markers with counts; zooming in splits clusters.
- High zoom: individual pins show photo thumbnails.
- Panning/zooming on 50k geotagged photos has no jank.

## Technical specs
- Cluster in the frontend (MapLibre GeoJSON source + clustering) or server-side aggregation in Rust — data volume is the deciding factor; 50k points is within MapLibre's clustered-GeoJSON capability.
- Thumbnails for pins served via `thumb://` scheme (§3 rule 3).
- Perf budget per §9: smooth pan/zoom on the 50k fixture in CI perf smoke tests.

## Testing criteria (TDD)
- E2e: 50k fixture → clusters visible at world zoom; zoom-in resolves to thumbnails; no dropped frames budget exceeded.
- Unit: cluster aggregation correctness at a zoom boundary (counts match photo totals).

## Acceptance criteria
- [ ] 06-002: Pins cluster at low zoom, resolve to photo thumbnails on zoom-in.
- [ ] 06-002: 50k-photo library pans/zooms without jank.
