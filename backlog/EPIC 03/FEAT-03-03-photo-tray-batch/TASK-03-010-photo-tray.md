# task: photo tray — cross-folder selection with hold pinning

**Feature:** [FEAT-03-03-photo-tray-batch](FEAT-03-03-photo-tray-batch.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in progress (2026-10-28)

## Description
Picasa-style Photo Tray: a temporary cross-folder selection pinned at the bottom of the window; actions (export, batch ops) apply to tray contents; "Hold" pins items (PLAN §2 B "Photo Tray", §6 P2a).

## Functional specs
- Adding photos to the tray works from any folder; the tray persists while browsing across folders.
- "Hold" pins items so they survive tray clears; unheld items clear with one click.
- Tray shows thumbnails and count; single-click a tray item jumps to it in the grid.
- All applicable actions (export, album add, batch ops) operate on the tray when it has contents.

## Technical specs
- Frontend state (Svelte store) — tray is session-scoped UI state, not catalog data; no sidecar writes.
- Tray contents are photo file-ids; actions read the id list and dispatch to existing command paths.
- Tray panel component at window bottom, keyboard shortcut to clear.

## Testing criteria (TDD)
- Unit: tray store — add/hold/clear semantics (held items survive clear, unheld don't).
- E2e: add two photos from different folders, clear unheld, apply album-add — only held members affected.

## Acceptance criteria
- [x] 03-010: tray holds a cross-folder selection while browsing.
- [x] 03-010: "Hold" pins items; clear removes only unheld.
- [x] 03-010: actions apply to tray contents.
