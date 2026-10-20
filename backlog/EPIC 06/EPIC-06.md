# EPIC-06 — Places (geotagging, map)

**Plan ref:** §6 Phase 3, Places track · **Size:** XL (track, parallel with 05/07) · **Depends on:** EPIC-03

## Acceptance

- Map view renders a 50k-photo library smoothly: clustered pins, no jank while panning/zooming, fully offline (PMTiles).

## feat: map infrastructure

- [ ] `06-001` (L) MapLibre GL JS panel + Protomaps PMTiles offline basemap; tile bundling strategy.
- [ ] `06-002` (M) Pins/clustering: markers cluster at low zoom, resolve to photo thumbnails on zoom-in.

## feat: map view & interaction

- [ ] `06-003` (M) Library-wide map view: pin per geotagged photo; current search/filters apply to the map.
- [ ] `06-004` (M) Click pin → open viewer; select pins → feed Photo Tray.

## feat: geotagging

- [ ] `06-005` (M) Drag-to-geotag on map: writes GPS to sidecar (`exif:GPSLatitude/Longitude`); optional embed on Save (per 00-002).
- [ ] `06-006` (M) Geotag tool: one photo, multi-selection, or whole folder/album in one pass.
- [ ] `06-007` (M) "Geotagged" filter.

## feat: reverse geocoding

- [ ] `06-008` (M) Embedded GeoNames cities dataset; offline reverse geocode → place names in catalog + FTS.
