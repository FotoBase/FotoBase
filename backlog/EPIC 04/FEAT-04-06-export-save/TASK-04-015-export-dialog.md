# task: build export dialog

**Feature:** [FEAT-04-06-export-save](FEAT-04-06-export-save.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** todo

## Description
Export dialog per the §2J/File-menu spec: original or custom resize (H/V pixel limit, slider + input), quality presets (Auto/Normal/Maximum/Minimum/Custom %), numbered filenames, optional text watermark, with edits baked in (PLAN §6 P2b Export).

## Functional specs
- Destination folder choice; size original or custom resize with horizontal/vertical pixel limit (slider + input box).
- Quality presets: Auto, Normal, Maximum, Minimum, Custom (% slider).
- Optional numbered filenames to preserve order; optional text watermark (name/domain/copyright).
- Exports include baked-in edits; multi-selection exports batched with progress.

## Technical specs
- `crates/export` pipeline: decode → apply recipe at full res (shares 04-018 engine) → resize (libvips) → encode.
- Watermark = text rendered onto the output pixels at export time (not a recipe op).
- Numbering: ordered by current sort, `name-1…` with collision avoidance.

## Testing criteria (TDD)
- Integration: export with recipe → output pixels match golden full-res render within tolerance.
- Unit: resize limit honored on both dimensions (landscape + portrait fixtures).
- Integration: numbered filenames sequential and collision-free; watermark present in output bytes.

## Acceptance criteria
- [ ] 04-015: Export dialog offers resize (H/V limit), quality presets, numbered filenames, text watermark; edits baked in.
