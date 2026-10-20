# task: build pro tab recipe ops

**Feature:** [FEAT-10-02-f2-advanced-editor](FEAT-10-02-f2-advanced-editor.md) · **Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (deferred)

## Description
Pro tab on the recipe engine: curves, levels, HSL, exposure, vibrance, perspective/keystone, Lensfun lens correction, noise reduction (PLAN §7 F2 item 2). Every tool is just another recipe op — non-destructive, same engine as EPIC-04.

## Functional specs
- Each pro tool works non-destructively: adjustments recorded in the recipe, original untouched.
- Tools are stackable/reorderable like existing EPIC-04 recipe ops.
- Lens correction picks the right lens profile from EXIF via the Lensfun DB.

## Technical specs
- Implement as recipe ops in the EPIC-04 engine (no separate editor path); Lensfun DB for lens correction.
- UI is a new tab in the existing editor, not a new window/app.

## Testing criteria (TDD)
- unit: each op has a golden-image test (known input → expected output) matching the EPIC-04 golden-test harness.

## Acceptance criteria
- [ ] F2-002: Curves, levels, HSL, exposure, vibrance, keystone, Lensfun correction, and noise reduction exist as recipe ops with golden-image tests — non-destructive.
