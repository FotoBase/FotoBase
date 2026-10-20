# task: implement undo, redo, and per-photo edit history

**Feature:** [FEAT-04-05-edit-recipe-engine](FEAT-04-05-edit-recipe-engine.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** todo

## Description
Per-photo edit history with undo, redo, undo all, and revert, matching the Picasa workflow (PLAN §2E "Undo/Redo/Undo All; Revert"; §6 P2b).

## Functional specs
- Each edit pushes onto the photo's history; undo steps back one op, redo re-applies, undo all clears pending edits, revert returns to original at any time.
- History is per-photo; switching photos preserves each photo's own history.
- Undoing never touches the original file.

## Technical specs
- History = stack of recipe states in the editor session; persisted state is just the current recipe (sidecar), history itself is session-scoped.
- Revert = drop recipe / restore last saved recipe; "Undo all edits" clears the recipe (§2 Picture context menu).

## Testing criteria (TDD)
- Unit: apply 3 ops → undo×2 → redo×1 → recipe equals expected op list.
- Integration: revert after a series of edits leaves the original byte-identical (checksum, §9 Data safety).

## Acceptance criteria
- [ ] 04-003: Undo/redo/undo-all/revert work with per-photo history.
- [ ] 04-003: Originals untouched throughout (checksummed in tests).
