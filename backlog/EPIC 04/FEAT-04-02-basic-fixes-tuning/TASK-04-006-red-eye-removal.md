# task: implement red-eye removal

**Feature:** [FEAT-04-02-basic-fixes-tuning](FEAT-04-02-basic-fixes-tuning.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** todo

## Description
Red-eye removal with auto and manual modes as a recipe op (PLAN §2E Basic Fixes; also a batch op per §2E batch list).

## Functional specs
- Auto mode detects and fixes red eyes in one click.
- Manual mode lets the user click/drag over an eye region to fix it.
- Result is a recipe op with live preview and golden-image coverage.

## Technical specs
- `red_eye` recipe op storing affected regions in params; detection heuristic over red-channel dominance in the region.
- Shared implementation between single-photo edit and batch "auto red-eye correction" (04-011).

## Testing criteria (TDD)
- Integration: fixture with red eyes → auto mode darkens pupils, golden image within tolerance.
- Unit: manual region fix produces localized change only — pixels outside region untouched.

## Acceptance criteria
- [ ] 04-006: Auto + manual red-eye removal work as recipe ops with live preview.
- [ ] 04-006: Golden-image test passes.
