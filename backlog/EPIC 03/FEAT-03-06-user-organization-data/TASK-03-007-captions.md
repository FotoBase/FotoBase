# task: captions written to dc:description

**Feature:** [FEAT-03-06-user-organization-data](FEAT-03-06-user-organization-data.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** todo

## Description
Per-photo caption stored in `dc:description` (PLAN §2 B Captions, §5 mapping), searchable via 03-013, editable inline. Depends on the sidecar merge-on-write design (00-010, hard gate for EPIC-03).

## Functional specs
- Caption editable from the viewer/info pane; visible under thumbnails when the caption mode is selected (03-015).
- Captions are searchable as-you-type and survive rebuild.
- Multi-line captions supported; empty caption clears the field cleanly.

## Technical specs
- `dc:description` via the `metadata` crate merge-on-write; write-through updates catalog + FTS (03-002/03-013).
- Sidecar is source of truth; catalog row derived.

## Testing criteria (TDD)
- Unit: caption write produces the expected `dc:description` fragment; clearing removes it per merge rules.
- Integration: caption → search finds it → rebuild → caption + searchability survive.

## Acceptance criteria
- [ ] 03-007: captions land in `dc:description` and are searchable.
- [ ] 03-007: captions survive catalog rebuild.
- [ ] 03-007: originals unchanged (checksum).
