# feat: Basic Fixes & Tuning tabs

**Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** planned

## Acceptance criteria

- Crop/straighten, red-eye (auto + manual), I'm Feeling Lucky, Auto Contrast/Color all work as recipe ops with live preview at interactive speeds.
- Tuning sliders (Fill Light, Highlights, Shadows, Color Temperature) and the neutral-color picker update the preview without perceptible lag.
- Each op has a golden-image test; the edit view shows tabs + filmstrip and never modifies the original file.

- [x] `04-005` (M) Crop (free + preset aspect ratios) and Straighten (small-angle rotate, auto-crop).
- [ ] `04-006` (M) Red-eye removal (auto + manual).
- [ ] `04-007` (M) "I'm Feeling Lucky" one-click; Auto Contrast; Auto Color.
- [ ] `04-008` (M) Tuning tab: Fill Light, Highlights, Shadows, Color Temperature sliders; neutral-color picker (white balance).
- [ ] `04-009` (M) Starter effects: Sharpen, Sepia, B&W, Saturation (strength slider).
- [ ] `04-010` (M) Edit view UI: single photo, edit tabs, filmstrip, live preview.

## Tasks

- [ ] [TASK-04-005-crop-straighten.md](TASK-04-005-crop-straighten.md) — implement crop and straighten ops
- [ ] [TASK-04-006-red-eye-removal.md](TASK-04-006-red-eye-removal.md) — implement red-eye removal
- [ ] [TASK-04-007-auto-fixes.md](TASK-04-007-auto-fixes.md) — implement one-click auto fixes
- [ ] [TASK-04-008-tuning-sliders.md](TASK-04-008-tuning-sliders.md) — implement tuning tab sliders and neutral picker
- [ ] [TASK-04-009-starter-effects.md](TASK-04-009-starter-effects.md) — implement starter effects
- [ ] [TASK-04-010-edit-view-ui.md](TASK-04-010-edit-view-ui.md) — build edit view ui
