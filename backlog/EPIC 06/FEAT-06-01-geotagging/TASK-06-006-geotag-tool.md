# task: build geotag tool

**Feature:** [FEAT-06-01-geotagging](FEAT-06-01-geotagging.md) · **Epic:** [EPIC-06 — Places](../EPIC-06.md) · **Status:** todo

## Description
Tools → Geotag: tag one photo, a multi-selection, or a whole folder/album in one pass, with undo (PLAN §2 Tools menu "Geotag"; §6 P3 Places).

## Functional specs
- Works on single photo, tray multi-selection, or entire folder/album.
- One pass applies the chosen location to all targets; progress shown for large sets.
- Undo (catalog trash / undo window per EPIC-03) restores prior GPS values.

## Technical specs
- Reuses the drag-to-geotag sidecar write path (06-005); batch = job-queue writes at background priority.
- Undo: previous sidecar GPS values snapshot before batch write (sidecar backup per §5 merge-on-write).

## Testing criteria (TDD)
- Integration: geotag a folder fixture → every photo's sidecar has the GPS; originals untouched.
- Integration: undo → all sidecars restored to previous GPS values.
- E2e: Tools → Geotag on album selection completes with progress.

## Acceptance criteria
- [ ] 06-006: Geotag tool handles one photo, multi-selection, or whole folder/album in one pass, with undo.
