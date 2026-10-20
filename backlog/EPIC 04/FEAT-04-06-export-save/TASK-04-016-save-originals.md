# task: implement save with originals backup

**Feature:** [FEAT-04-06-export-save](FEAT-04-06-export-save.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** todo

## Description
Explicit "Save" per the §F spec: write edited pixels to the original path, move the untouched original to a sibling `.originals/` folder, record the saved file as a stacked version; Revert restores from `.originals/` (PLAN §2F).

## Functional specs
- Save renders the recipe at full resolution and writes it to the original path.
- The untouched original moves to `.originals/` next to the file; Revert restores it byte-identical at any time.
- The saved file is recorded as a stacked version (EPIC-03 stacking).
- Save is the only path that ever modifies an original (§3 rule 1).

## Technical specs
- Full-res render via the shared recipe engine (04-018); atomic write (temp file + rename) to avoid data loss.
- Metadata sidecar kept alongside; the 00-002 opt-in "embed XMP in original on Save" rule applies.
- Stack record in catalog + sidecar per EPIC-03 conventions.

## Testing criteria (TDD)
- Integration: Save → edited bytes at original path, original byte-identical in `.originals/`; Revert restores it (checksums, §9 Data safety).
- Integration: Save interrupted (kill mid-write) → original intact via atomic-write temp file.
- Integration: saved file appears as stacked version in the grid.

## Acceptance criteria
- [ ] 04-016: Save writes edited pixels to original path, original → `.originals/`, recorded as stacked version.
- [ ] 04-016: Revert restores the original byte-identical.
