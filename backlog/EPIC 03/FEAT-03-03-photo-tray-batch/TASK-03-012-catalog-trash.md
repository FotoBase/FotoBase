# task: catalog trash — soft-delete with undo window

**Feature:** [FEAT-03-03-photo-tray-batch](FEAT-03-03-photo-tray-batch.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in review (2026-10-28)

## Description
Catalog trash: soft-delete plus an undo window for remove-from-album, hide, and unlink actions (PLAN §6 P2a) — reversible organizational mistakes without touching disk.

## Functional specs
- Remove-from-album, hide, and unlink land in a trash view instead of vanishing; undo restores exactly the prior state.
- Undo window: actions remain undoable for a configurable period (default e.g. 30 days), then expire.
- Trash view lists pending items with the action, timestamp, and photo; undo-all per action type.
- Nothing hits disk — soft-deletes are catalog/user-data records only.

## Technical specs
- Trash table in the catalog (action, target ids, timestamp, restore payload as JSON); expiry by timestamp sweep on startup.
- Undo applies the reverse operation through the same mutation paths (album add, unhide, relink) so sidecars stay consistent.
- Unlink = remove folder from library (P1 "remove from catalog") reworked through trash.

## Testing criteria (TDD)
- Unit: each action type's restore payload reverses it exactly (fixture round-trip).
- Integration: hide → undo → photo visible again with stars/tags intact; expiry sweep drops entries past the window.

## Acceptance criteria
- [x] 03-012: remove-from-album, hide, unlink are soft-deleted with undo.
- [x] 03-012: undo restores exact prior state; window is configurable.
- [x] 03-012: no disk writes occur (originals untouched).
