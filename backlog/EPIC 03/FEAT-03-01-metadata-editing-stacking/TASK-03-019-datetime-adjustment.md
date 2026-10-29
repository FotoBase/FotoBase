# task: date/time adjustment — shift, timezone fix, multi-camera merge

**Feature:** [FEAT-03-01-metadata-editing-stacking](FEAT-03-01-metadata-editing-stacking.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in review (2026-10-29)

## Description
Let the user shift capture time, fix timezones, and merge multi-camera sets so chronology is consistent; adjusted time is written to the sidecar so it survives catalog rebuild (PLAN §6 P2a deliverable). Depends on the sidecar merge-on-write design doc (task 00-010, hard gate for EPIC-03).

## Functional specs
- Shift capture time by a delta (hours/days) on a selection; preview before applying.
- Fix a wrong timezone offset on a selection (e.g. camera left on UTC) without changing the displayed instant incorrectly.
- Merge multi-camera sets: align two cameras' clocks by offsetting one set to match the other.
- Adjusted time affects sort order everywhere; rebuildable from sidecars.

## Technical specs
- Write adjusted time to XMP in the sidecar (standard EXIF/XMP date-time fields via the `metadata` crate); catalog `files.taken_at` is derived cache updated on write-through (03-002).
- Batch operation runs as a job-queue task with progress; originals untouched (PLAN §3 rule 1).
- UI: date/time dialog reachable from info pane and multi-select context menu.

## Testing criteria (TDD)
- Unit: shifting a +02:00 photo by -2h yields the expected stored datetime; sort order of a fixture set matches the expected sequence after merge.
- Integration: after adjustment, delete catalog → rebuild → taken_at values survive (sidecar is source of truth).
- E2e: multi-camera fixture shows interleaved chronological order after merge.

## Acceptance criteria
- [x] 03-019: capture-time shift, timezone fix, and multi-camera merge produce consistent ordering.
- [x] 03-019: adjustments are written to sidecars and rebuildable.
- [x] 03-019: originals unchanged (checksum).
