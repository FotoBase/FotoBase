# task: add heal brush retouch recipe op

**Feature:** [FEAT-08-05-remaining-editor-tools](FEAT-08-05-remaining-editor-tools.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
Retouch heal brush for blemishes/dust, recorded as strokes in a recipe op (PLAN §2 §E Retouch P4; §5 recipe op `retouch(strokes)`). Non-destructive and consistent between preview and export — strokes are stored, healing is re-executed at render time.

## Functional specs
- Painting over a blemish heals it from surrounding pixels; stroke size adjustable.
- Strokes are recorded, shown as an editable stroke list; individual strokes can be deleted.
- Preview equals export (feature acceptance); healing re-runs identically at full resolution.
- Undo removes the last stroke; "Undo all edits" clears the op (EPIC-04).

## Technical specs
- Recipe op `{ type: "retouch", params: { strokes: [{ x, y, radius, points[] }] } }` in `fb:Recipe` — coordinates normalized to image dimensions so resolution-independent re-render works.
- `render` crate: heal algorithm — classic diffusion-based heal (like Picasa's) or a small inpainting pass; deterministic and same-code preview/export.
- UI: canvas stroke capture in the edit view, stroke list panel.

## Testing criteria (TDD)
- Unit: healing a synthetic blemish (solid dot on gradient) converges close to the gradient.
- Integration: recipe with retouch applied at preview and export resolutions — visually equivalent within tolerance (golden image).
- Integration: deleting one stroke from the list re-renders without the other strokes changing.

## Acceptance criteria
- [ ] 08-002: Heal brush records strokes as recipe ops — non-destructive, consistent between preview and export.
- [ ] 08-002: Strokes are individually undoable/deletable and resolution-independent.
