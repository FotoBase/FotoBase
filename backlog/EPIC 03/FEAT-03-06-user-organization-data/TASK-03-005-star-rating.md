# task: star/rating written to xmp:Rating with starred filter

**Feature:** [FEAT-03-06-user-organization-data](FEAT-03-06-user-organization-data.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in review (2026-10-28)

## Description
Star (and opt-in 1–5 rating, per the 00-001 decision) written to `xmp:Rating` with a starred filter (PLAN §5: star = 5, or 1–5 if ratings enabled). Depends on the sidecar merge-on-write design doc (00-010, hard gate for EPIC-03).

## Functional specs
- Star toggle on a photo (grid or viewer); with ratings enabled, 1–5 selectable.
- Star/rating survives catalog rebuild (delete → rebuild → still starred).
- Starred filter (03-014's button) driven by this data.
- Originals untouched — value lives only in the sidecar.

## Technical specs
- `xmp:Rating` write via the `metadata` crate merge-on-write path (03-001); write-through on every toggle (03-002).
- Catalog stores a derived rating column for fast filtering; sidecar wins on conflict (§5 authority split).
- Ratings opt-in in General options (00-001 decision: default is star-only).

## Testing criteria (TDD)
- Unit: star toggle writes `xmp:Rating=5` sidecar fragment; unstar removes/clears it per merge rules.
- Integration: star → delete catalog → rebuild → photo still starred.
- Unit: given a photo with `xmp:Rating=5`, the starred filter returns it.

## Acceptance criteria
- [x] 03-005: star/rating writes `xmp:Rating` and survives catalog rebuild.
- [x] 03-005: starred filter works.
- [x] 03-005: originals unchanged (checksum).
