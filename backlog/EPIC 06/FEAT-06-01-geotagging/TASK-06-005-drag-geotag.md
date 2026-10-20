# task: implement drag-to-geotag on map

**Feature:** [FEAT-06-01-geotagging](FEAT-06-01-geotagging.md) · **Epic:** [EPIC-06 — Places](../EPIC-06.md) · **Status:** todo

## Description
Drag a photo onto the map to geotag it: GPS coordinates written to the sidecar (`exif:GPSLatitude`/`exif:GPSLongitude`), with optional embedding into the original on Save per the 00-002 opt-in rule (PLAN §2H "drag-and-drop to geotag"; §5 XMP data model; §6 P3 Places).

## Functional specs
- Dragging a photo (or tray selection) onto the map sets its GPS in the sidecar; originals untouched otherwise (§3 rule 1).
- If the 00-002 "embed XMP in original on Save" setting is on, Save writes GPS into the file too.
- Geotags survive catalog rebuild (sidecar is source of truth).

## Technical specs
- Sidecar write via `crates/metadata` merge-on-write (§5); GPS fields per §5 table.
- Map drop-target in the Places panel; lat/lon from MapLibre event → XMP EXIF GPS (incl. sign/ref conventions).

## Testing criteria (TDD)
- Integration: drag-drop on a fixture → sidecar contains correct `exif:GPSLatitude/Longitude`; original checksum unchanged.
- Integration: catalog rebuild → GPS preserved.
- Unit: lat/lon → XMP GPS value round-trip (positive/negative, hemispheres).

## Acceptance criteria
- [ ] 06-005: Drag-to-geotag writes GPS to sidecar; embed-on-Save follows the 00-002 opt-in rule.
- [ ] 06-005: Originals otherwise untouched; geotags survive rebuild.
