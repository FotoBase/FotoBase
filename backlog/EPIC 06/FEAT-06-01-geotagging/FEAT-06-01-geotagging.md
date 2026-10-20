# feat: geotagging

**Epic:** [EPIC-06 — Places](../EPIC-06.md) · **Status:** planned

## Acceptance criteria

- Drag-to-geotag writes GPS to the sidecar (`exif:GPSLatitude/Longitude`); embed-on-Save follows the 00-002 opt-in rule; originals otherwise untouched.
- Geotag tool handles single photo, multi-selection, and whole folder/album in one pass with undo.
- "Geotagged" filter works in grid and map; geotags survive catalog rebuild.

- [ ] `06-005` (M) Drag-to-geotag on map: writes GPS to sidecar (`exif:GPSLatitude/Longitude`); optional embed on Save (per 00-002).
- [ ] `06-006` (M) Geotag tool: one photo, multi-selection, or whole folder/album in one pass.
- [ ] `06-007` (M) "Geotagged" filter.

## Tasks

- [ ] [TASK-06-005-drag-geotag.md](TASK-06-005-drag-geotag.md) — implement drag-to-geotag on map
- [ ] [TASK-06-006-geotag-tool.md](TASK-06-006-geotag-tool.md) — build geotag tool
- [ ] [TASK-06-007-geotagged-filter.md](TASK-06-007-geotagged-filter.md) — add geotagged filter
