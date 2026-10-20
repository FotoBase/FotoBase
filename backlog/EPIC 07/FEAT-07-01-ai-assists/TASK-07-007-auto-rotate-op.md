# task: add auto-rotate horizon leveling recipe op

**Feature:** [FEAT-07-01-ai-assists](FEAT-07-01-ai-assists.md) · **Epic:** [EPIC-07 — Smart search](../EPIC-07.md) · **Status:** todo

## Description
AI-estimate the horizon angle and offer auto-rotate/leveling as a recipe op (PLAN §2 2026 additions: "Auto-rotate / level horizon: AI-estimated rotation applied as a recipe op"). Non-destructive: preview equals export because it's just another op in the EPIC-04 recipe engine.

## Functional specs
- "Auto-level" action estimates the rotation needed and shows a live preview at screen resolution before applying.
- Applying it appends a recipe op to the sidecar recipe (`fb:Recipe`, PLAN §5) — the original is untouched.
- If no confident horizon is found, the action says so and does nothing rather than guessing.
- The op is undoable like every recipe op (EPIC-04 undo/redo).

## Technical specs
- `ai` crate: horizon estimation — edge/orientation-based line detection (e.g. Hough-style over downscaled grayscale) or a small ONNX model behind the same interface; keep the estimate a single angle ± confidence.
- `render`/recipe engine (EPIC-04): new op `{ type: "auto_rotate", params: { angle_deg } }` — same pure function for preview and export (PLAN §5 recipe model); reuse the existing `straighten` rotate+auto-crop math.
- Runs as a job-queue task when invoked on a selection.

## Testing criteria (TDD)
- Unit: synthetic image with a known 5° tilted horizon → estimated angle within tolerance, confidence above threshold.
- Integration (golden image): applying the op at preview resolution and export resolution yields identical results within tolerance (PLAN §9 render fidelity).
- Unit: flat-cloud sky image (no horizon) → low confidence → no op applied.

## Acceptance criteria
- [ ] 07-007: Auto-rotate/horizon leveling is a recipe op — non-destructive, preview = export.
- [ ] 07-007: Low-confidence estimates never apply an op silently.
