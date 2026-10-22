# task: build info pane — EXIF, histogram, file properties

**Feature:** [FEAT-01-07-ui-viewer-info](FEAT-01-07-ui-viewer-info.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in progress (2026-10-22)

## Description
Implement the info pane per PLAN §2D ("Info pane: EXIF (camera, lens, exposure), histogram, file properties") and §2 View menu ("Show image properties" toggle). Updates per photo as the viewer navigates.

## Functional specs
- Shows camera, lens, exposure (aperture/shutter/ISO) from EXIF, plus file properties (path, size, dimensions, format, modified date).
- Histogram computed from the displayed image; correct for the shown photo.
- Toggle via View → Show image properties; pane updates live as photos change.

## Technical specs
- EXIF read through the `metadata` crate (exiv2 bindings, §4); values cached in the `metadata` table at scan time where available.
- Histogram computed in the frontend from the rendered preview (downsampled bins are fine) or in `render` — no full-res trips to the UI.

## Testing criteria (TDD)
- Unit: given a fixture EXIF payload, the pane model reports camera/lens/exposure correctly.
- E2E: open two fixture photos ⇒ histogram and EXIF swap with each photo.

## Acceptance criteria
- [x] 01-020: Info pane shows EXIF (camera, lens, exposure), a correct histogram, and file properties, updating per photo.
