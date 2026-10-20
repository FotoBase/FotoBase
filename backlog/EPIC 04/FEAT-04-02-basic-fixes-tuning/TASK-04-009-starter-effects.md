# task: implement starter effects

**Feature:** [FEAT-04-02-basic-fixes-tuning](FEAT-04-02-basic-fixes-tuning.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** todo

## Description
Starter effects — Sharpen, Sepia, B&W, Saturation (with strength slider) — as recipe ops (PLAN §6 P2b "Starter effects"; §2E Effects, P2 items).

## Functional specs
- One-click effects from the effects tab; Saturation (and where applicable others) expose a strength slider.
- Effects are recipe ops: undoable, live-previewed, identical at export resolution.

## Technical specs
- `sharpen`, `sepia`, `bw`, `saturation` ops in the 00-011 model; strength = numeric param.
- B&W/sepia also exist as view-only display modes in EPIC-03 (§2 View menu) — keep the recipe op separate from the view mode.

## Testing criteria (TDD)
- Integration: golden-image test per effect at default and mid-strength params.
- Unit: strength 0 leaves the buffer unchanged.

## Acceptance criteria
- [ ] 04-009: Sharpen, Sepia, B&W, Saturation available as recipe ops with strength slider.
- [ ] 04-009: Golden-image tests pass for each effect.
