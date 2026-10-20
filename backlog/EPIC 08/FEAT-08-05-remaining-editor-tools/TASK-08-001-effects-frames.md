# task: add remaining effects and frames recipe ops

**Feature:** [FEAT-08-05-remaining-editor-tools](FEAT-08-05-remaining-editor-tools.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
Implement the remaining Picasa effects as recipe ops on the EPIC-04 recipe engine: Warmify, Film Grain, Tint, Soft Focus, Vignette, plus frames Border and Drop Shadow (PLAN §2 §E effects tabs, §6 P4). Each is a versioned op with a strength parameter where applicable; golden-image tested.

## Functional specs
- Each effect appears in the Effects tab with a strength slider (one-click default where Picasa had none).
- Effects apply non-destructively as recipe ops; undo/redo/revert work as with all ops.
- Preview at screen resolution matches export at full resolution (feature acceptance).
- Effects compose with each other and with existing P2b ops in one ordered recipe.

## Technical specs
- New ops in the recipe engine (`render` crate, EPIC-04): `{ type: "warmify" | "film_grain" | "tint" | "soft_focus" | "vignette" | "border" | "drop_shadow", params: { strength, color, … } }` — pure functions over the linear-light buffer (PLAN §5).
- Film grain: deterministic seeded noise so preview/export and re-render are reproducible.
- Soft focus / vignette: separable or radial passes, no per-pixel allocations in the hot path.
- Golden-image tests with per-pixel tolerance per op (PLAN §9 render fidelity).

## Testing criteria (TDD)
- Unit: each op with default params transforms a fixture pixel region as the golden image expects (write goldens first).
- Integration: same recipe rendered at preview and export resolution — results within tolerance.
- Unit: seeded grain op is reproducible across runs.

## Acceptance criteria
- [ ] 08-001: Warmify, Film Grain, Tint, Soft Focus, Vignette, Border, Drop Shadow all work as recipe ops with golden-image tests.
- [ ] 08-001: All effects compose in one recipe with working undo/redo.
