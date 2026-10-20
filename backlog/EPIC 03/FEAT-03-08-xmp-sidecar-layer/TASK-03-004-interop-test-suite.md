# task: darktable/digiKam interop test suite in CI

**Feature:** [FEAT-03-08-xmp-sidecar-layer](FEAT-03-08-xmp-sidecar-layer.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** todo

## Description
CI suite round-tripping our sidecars through darktable and digiKam, verifying tags, ratings, captions, and GPS survive both directions (PLAN §9 Interop row; P2a acceptance "darktable/digiKam show tags, ratings, captions from our sidecars").

## Functional specs
- Fotobase-written sidecars read correctly in darktable and digiKam (fields visible with expected values).
- Sidecars edited by either tool re-read into Fotobase without loss of the other tool's fields (merge-on-write proof, 03-001).
- Suite runs in CI on every change to the metadata layer.

## Technical specs
- Headless interop: run digiKam/darktable CLI in containers in CI where feasible; where headless isn't possible, fall back to scripted XMP fixtures produced *by* the real tools (checked in) — same guarantee, no live GUI dependency.
- Cover: `xmp:Rating`, `dc:subject`, `lr:hierarchicalSubject`, `dc:description`, `exif:GPSLatitude/Longitude` (PLAN §5 mapping).
- Assertions compare parsed XMP, not bytes (tool ordering differs).

## Testing criteria (TDD)
- Integration: rating/tags/caption/GPS written by Fotobase parse identically when read by the digiKam-produced reader path and vice versa.
- Integration: a digiKam-edited sidecar re-read by Fotobase keeps digiKam's custom fields after our next write.

## Acceptance criteria
- [ ] 03-004: interop suite round-trips tags, ratings, captions, GPS through darktable and digiKam.
- [ ] 03-004: suite runs in CI (live or fixture-based) on metadata-layer changes.
- [ ] 03-004: merge-on-write proven against real third-party output.
