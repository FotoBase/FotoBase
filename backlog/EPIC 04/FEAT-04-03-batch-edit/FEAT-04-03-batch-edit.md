# feat: batch edit

**Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** planned

## Acceptance criteria

- All §2E batch ops apply to a multi-selection via the Photo Tray with progress and cancellation.
- Batch results equal per-photo results (same op → same pixels).
- Pattern rename with date/resolution suffixes produces collision-free names; originals untouched except explicit Save.

- [ ] `04-011` (M) Batch ops per §2E: rotate CW/CCW, auto contrast/color, sepia, sharpen, warmify, film grain, B&W, auto red-eye, show/hide text.
- [ ] `04-012` (S) Rename with custom name + optional date/resolution suffixes.

## Tasks

- [ ] [TASK-04-011-batch-ops.md](TASK-04-011-batch-ops.md) — implement batch edit ops
- [ ] [TASK-04-012-batch-rename.md](TASK-04-012-batch-rename.md) — implement pattern rename
