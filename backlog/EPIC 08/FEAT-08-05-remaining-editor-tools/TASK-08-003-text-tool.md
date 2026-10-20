# task: add text overlay recipe op

**Feature:** [FEAT-08-05-remaining-editor-tools](FEAT-08-05-remaining-editor-tools.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
Text tool overlaying text with font, size, color, outline, transparency, recorded as a recipe op (PLAN §2 §E Text P4; §5 op `text`). Rendering identical at screen and export resolution (feature acceptance).

## Functional specs
- User places text on the photo and edits font, size, color, outline (width+color), and transparency.
- Text is draggable and re-editable after placement (op params are editable, not baked pixels).
- Preview matches export rendering exactly at both resolutions.
- Works in batch edit ("show text / hide text", PLAN §2 Picture context menu batch edit list).

## Technical specs
- Recipe op `{ type: "text", params: { text, x, y, font, size, color, outline, transparency, anchor } }` in `fb:Recipe`; position normalized (fractions of width/height) so it's resolution-independent.
- `render` crate: text rasterization via a cross-platform font stack (system fonts, e.g. `fontdue`/`rusttype`-class loader); identical glyph rendering path for preview and export — only scale differs.
- Font availability check: missing font falls back to a bundled default and records the substitution.

## Testing criteria (TDD)
- Unit: text op with default params renders the golden image at both preview and export scale (write goldens first — same glyphs, proportional position).
- Unit: transparency 1.0 vs 0.5 produce expected alpha blend on a solid background.
- Integration: re-editing op params re-renders without other ops changing.

## Acceptance criteria
- [ ] 08-003: Text tool supports font, size, color, outline, transparency; rendering identical at screen and export resolution.
- [ ] 08-003: Text ops are re-editable and resolution-independent (normalized coordinates).
