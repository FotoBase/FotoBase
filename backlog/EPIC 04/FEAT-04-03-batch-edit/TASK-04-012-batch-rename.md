# task: implement pattern rename

**Feature:** [FEAT-04-03-batch-edit](FEAT-04-03-batch-edit.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** todo

## Description
Rename selected files to a custom name with optional picture-date and picture-resolution suffixes (PLAN §2 File menu "Rename"; §6 P2b), producing collision-free names.

## Functional specs
- Dialog: custom name + checkboxes for date and resolution suffixes.
- Multi-selection renames to numbered pattern (`name-1, name-2…`, §2B "Batch rename").
- Never overwrites an existing file; originals' bytes untouched (rename only); catalog follows the rename (hash re-association per §5).

## Technical specs
- File rename via `crates/scanner`/library service; catalog entries updated in the same transaction (path change, hash unchanged).
- Collision handling: append counter until free.

## Testing criteria (TDD)
- Integration: rename 3 files with date+resolution suffixes → expected names, no collisions.
- Unit: target name already exists → next free name chosen, no overwrite.
- Integration: catalog lookup by new name resolves the same photo (hash re-association).

## Acceptance criteria
- [ ] 04-012: Rename with custom name + optional date/resolution suffixes produces collision-free names.
- [ ] 04-012: Originals untouched except the rename; catalog stays consistent.
