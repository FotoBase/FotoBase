# task: view-time sepia and B&W display modes

**Feature:** [FEAT-03-07-view-options](FEAT-03-07-view-options.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in progress (2026-10-28)

## Description
View-time display modes (sepia, B&W) applied at display only — no recipe, no sidecar write, originals unchanged (PLAN §6 P2a "display modes (sepia/B&W view modes)").

## Functional specs
- Toggle sepia / B&W on the viewer; applies to the displayed photo immediately.
- No edit recipe is created, no sidecar written, original checksum unchanged after use.
- Toggling off restores normal color instantly (no re-decode roundtrip penalty).

## Technical specs
- Frontend CSS filters (`filter: grayscale(1)`, `sepia(...)`-style curve) on the preview element — plan implies view-only modes, so the cheap path is correct; no render-crate involvement.
- Mode is transient view state, not persisted.

## Testing criteria (TDD)
- E2e: toggle each mode → applied; toggle off → restored; session with modes used → sidecar dir unchanged, original checksums identical.
- Unit (trivial guard): mode state machine never emits a recipe op.

## Acceptance criteria
- [x] 03-018: sepia and B&W display modes work in the viewer.
- [x] 03-018: no recipe, no sidecar write; originals unchanged (checksum).
