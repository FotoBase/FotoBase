# task: implement batch edit ops

**Feature:** [FEAT-04-03-batch-edit](FEAT-04-03-batch-edit.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** todo

## Description
Apply the §2E batch ops to a multi-selection via the Photo Tray: rotate CW/CCW, auto contrast, auto color, sepia, sharpen, warmify, film grain, B&W, auto red-eye, show/hide text — with progress and cancellation (PLAN §2E "Batch edit"; §6 P2b).

## Functional specs
- All §2E batch ops available from picture context menu / tray actions on a multi-selection.
- Progress reported; run cancellable mid-batch; partially applied ops remain valid recipes.
- Batch result equals per-photo result: same op → same pixels.
- Show/hide text toggles the text overlay op in each selected recipe.

## Technical specs
- Reuses the single-photo recipe ops from FEAT-04-02 (rotate is a new op); batch = queue N per-photo recipe updates through the job queue (§3 rule 4), background priority.
- Depends on Photo Tray (EPIC-03) and the recipe engine (04-001).

## Testing criteria (TDD)
- Integration: batch-apply an op to 3 fixtures → each output equals applying the op per-photo (pixel comparison).
- Integration: cancel mid-batch → processed photos keep their recipes, unprocessed untouched.
- E2e: select via tray, apply batch op, observe progress UI.

## Acceptance criteria
- [ ] 04-011: All §2E batch ops apply to a multi-selection with progress and cancellation.
- [ ] 04-011: Batch results equal per-photo results.
