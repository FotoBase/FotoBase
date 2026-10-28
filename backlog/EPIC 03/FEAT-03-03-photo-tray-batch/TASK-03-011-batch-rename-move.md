# task: batch rename and batch move

**Feature:** [FEAT-03-03-photo-tray-batch](FEAT-03-03-photo-tray-batch.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in review (2026-10-28)

## Description
Batch rename to a pattern and batch move between folders, performing real file moves with catalog and sidecars following (PLAN §2 B "Batch rename / move", §6 P2a). Depends on the move/rename re-association design (00-010, hard gate for EPIC-03).

## Functional specs
- Rename a multi-selection to a pattern (`name-1, name-2…` with counters, dates, original stem); collisions detected and refused or auto-resolved per user choice.
- Move/drag photos between folders = real file moves; originals' bytes unchanged, only location/name.
- Catalog entries and sidecars follow the files; nothing is re-scanned as "new".
- Undo within the trash/undo window (03-012) reverses a batch rename/move.

## Technical specs
- Pattern engine: tokens for stem, counter (start/step/pad), taken_at date; preview before apply.
- Sidecar moves with the original (rename `IMG.xmp` alongside `IMG.jpg`); catalog path/mtime updated in the same transaction.
- Reuse the 03-003 re-association matching as the safety net if a sidecar is orphaned.
- File ops via the `scanner`/core layer with rollback on partial failure (abort remaining, report).

## Testing criteria (TDD)
- Unit: pattern rendering matches expected names; collision detection flags a fixture with a clashing target.
- Integration: batch move fixture → files + sidecars at new paths, catalog rows point to new paths, stars intact.
- Integration: interrupted move (kill mid-batch) leaves catalog consistent (aborted batch reported, no half-moved rows).

## Acceptance criteria
- [x] 03-011: batch rename applies patterns without collisions.
- [x] 03-011: batch move does real file moves; catalog and sidecars follow.
- [x] 03-011: originals' contents unchanged (checksum); no re-scan as new.
