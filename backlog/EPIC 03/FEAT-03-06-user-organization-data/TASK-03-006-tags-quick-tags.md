# task: tags, tag editor, and quick tags panel

**Feature:** [FEAT-03-06-user-organization-data](FEAT-03-06-user-organization-data.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** todo

## Description
Tags/keywords written to `dc:subject` (plus `lr:hierarchicalSubject` for nested tags), a tag editor, and a Quick Tags panel with 10 one-click buttons (PLAN §2 B Tags/Quick Tags, §5 mapping). Depends on the sidecar merge-on-write design (00-010, hard gate for EPIC-03).

## Functional specs
- Tag editor: add/remove/rename tags on a photo or selection; nested tags supported.
- Quick Tags panel: 10 user-configurable one-click buttons applying/removing a tag to the current selection.
- Tags are searchable (03-013) and survive rebuild; readable by darktable/digiKam.

## Technical specs
- `dc:subject` array + `lr:hierarchicalSubject` for nesting via the `metadata` crate; write-through on mutation.
- Catalog `tags`/`file_tags` tables (PLAN §5) as derived cache; tag rename rewrites affected sidecars as a batch job.
- Quick Tags config stored in app options; panel is a fixed 10-button strip.

## Testing criteria (TDD)
- Unit: adding tags produces expected `dc:subject` + `lr:hierarchicalSubject` fragments; nested tag renders `A|B` correctly.
- Integration: tag → rebuild → tag survives; tag rename across 3 fixture photos updates all 3 sidecars.
- E2e: Quick Tags button toggles a tag on a multi-selection in one click.

## Acceptance criteria
- [ ] 03-006: tags land in `dc:subject` (+ `lr:hierarchicalSubject`); nested tags work.
- [ ] 03-006: tag editor and Quick Tags panel (10 one-click buttons) work.
- [ ] 03-006: tags survive rebuild; originals unchanged (checksum).
