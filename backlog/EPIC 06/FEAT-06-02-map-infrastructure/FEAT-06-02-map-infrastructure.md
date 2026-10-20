# feat: map infrastructure

**Epic:** [EPIC-06 — Places](../EPIC-06.md) · **Status:** planned

## Acceptance criteria

- Map panel renders fully offline from bundled PMTiles basemap; no network requests ever (verified in tests).
- 50k-photo geotagged library: clustered pins render and pan/zoom without jank (epic acceptance).
- Clusters resolve into photo thumbnails as zoom increases, down to individual pins.

- [ ] `06-001` (L) MapLibre GL JS panel + Protomaps PMTiles offline basemap; tile bundling strategy.
- [ ] `06-002` (M) Pins/clustering: markers cluster at low zoom, resolve to photo thumbnails on zoom-in.

## Tasks

- [ ] [TASK-06-001-pmtiles-basemap.md](TASK-06-001-pmtiles-basemap.md) — set up offline pmtiles basemap
- [ ] [TASK-06-002-pin-clustering.md](TASK-06-002-pin-clustering.md) — implement map pin clustering
