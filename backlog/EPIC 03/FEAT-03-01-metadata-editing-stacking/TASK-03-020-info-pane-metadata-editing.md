# task: info-pane metadata editing — date, GPS, artist, copyright

**Feature:** [FEAT-03-01-metadata-editing-stacking](FEAT-03-01-metadata-editing-stacking.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in review (2026-10-29)

## Description
Editable fields in the info pane for date, GPS, artist, copyright, round-tripping through XMP sidecars per PLAN §5 (user data lives in sidecars; catalog is cache). Depends on the sidecar merge-on-write design doc (00-010, hard gate for EPIC-03).

## Functional specs
- Info pane exposes editable date, GPS (lat/lon), artist, copyright; save writes to the sidecar immediately.
- GPS entry accepts decimal degrees and DMS; invalid input is rejected inline.
- Edited values round-trip: re-read shows what was written; darktable/digiKam see the same values.

## Technical specs
- XMP mapping per PLAN §5: GPS → `exif:GPSLatitude`/`exif:GPSLongitude`; artist/copyright → standard XMP rights/creator fields (`dc:creator`, `dc:rights`) via the `metadata` crate.
- Write-through on every mutation (03-002 sync engine); originals untouched.
- Info pane (P1 deliverable) gains an edit mode; single photo only (batch covers 03-019).

## Testing criteria (TDD)
- Unit: each field's XMP write produces the expected sidecar XML fragment; DMS input parses to the correct decimal value.
- Integration: edit all four fields → re-read returns identical values; rebuild catalog → values survive.
- Interop: sidecar with edited fields round-trips through digiKam without loss (03-004 suite picks this up).

## Acceptance criteria
- [x] 03-020: date, GPS, artist, copyright editable in the info pane.
- [x] 03-020: all four round-trip through XMP and survive catalog rebuild.
- [x] 03-020: originals unchanged (checksum).
