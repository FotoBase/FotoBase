# task: implement one-click auto fixes

**Feature:** [FEAT-04-02-basic-fixes-tuning](FEAT-04-02-basic-fixes-tuning.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** todo

## Description
"I'm Feeling Lucky" one-click fix plus separate Auto Contrast and Auto Color one-clicks as recipe ops (PLAN §2E Basic Fixes; §6 P2b). Auto contrast/color are also batch ops (§2E).

## Functional specs
- I'm Feeling Lucky applies combined auto color + contrast in one click.
- Auto Contrast and Auto Color are available as separate one-click fixes.
- Each records a recipe op (not a baked-in change), undoable, previewable live.

## Technical specs
- `auto_contrast`, `auto_color` recipe ops; histogram-analysis based, deterministic given the input buffer.
- Shared implementation for batch edit variants (04-011).

## Testing criteria (TDD)
- Integration: each op on fixture images produces golden-expected output within tolerance.
- Unit: ops are deterministic — same input, same output across runs.

## Acceptance criteria
- [ ] 04-007: I'm Feeling Lucky, Auto Contrast, Auto Color work as one-click recipe ops.
- [ ] 04-007: Golden-image tests pass for each.
