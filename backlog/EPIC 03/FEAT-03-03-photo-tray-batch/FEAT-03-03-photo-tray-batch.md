# feat: photo tray & batch

**Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** planned

## Acceptance criteria

- Photo Tray holds a cross-folder selection; "Hold" pins items; all tray actions (export, batch ops) operate on the tray contents.
- Batch rename applies patterns without collisions; batch move performs real file moves with the catalog and sidecars following.
- Catalog trash soft-deletes (album removal, hide, unlink) with an undo window; nothing hits disk.

- [x] `03-010` (M) Photo Tray: cross-folder selection, "Hold" pinning, actions apply to tray.
- [x] `03-011` (M) Batch rename to pattern; batch move between folders (real file moves, catalog follows).
- [x] `03-012` (M) Catalog trash: soft-delete + undo window for remove-from-album, hide, unlink.

## Tasks

- [x] [TASK-03-010-photo-tray.md](TASK-03-010-photo-tray.md) — photo tray: cross-folder selection with hold pinning
- [x] [TASK-03-011-batch-rename-move.md](TASK-03-011-batch-rename-move.md) — batch rename and batch move
- [x] [TASK-03-012-catalog-trash.md](TASK-03-012-catalog-trash.md) — catalog trash: soft-delete with undo window
