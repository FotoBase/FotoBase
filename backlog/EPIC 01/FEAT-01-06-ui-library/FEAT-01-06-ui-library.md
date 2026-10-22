# feat: UI — library

**Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** planned

## Acceptance criteria

- Grid scrolls at 60 fps on the 50k fixture on all three OSes (DOM, with canvas fallback where 01-004 dictates).
- Zoom slider resizes thumbnails live; folder tree groups by year with working selection state and context menu.
- Folder Manager applies Scan Once / Scan Always / Remove and the library reflects the change immediately.
- Import copies files with chosen destination naming; duplicates skipped when "exclude duplicates" is on (default); live search filters filenames/folders while typing.

- [x] `01-013` (M) Virtualized thumbnail grid (TanStack Virtual) with zoom slider; DOM implementation.
- [x] `01-014` (M) Canvas-grid renderer (Linux default if 01-004 says so; fallback everywhere).
- [x] `01-015` (M) Folder tree grouped by year; selection state; context menu skeleton.
- [x] `01-016` (M) Folder Manager dialog: Scan Once / Scan Always / Remove from catalog (per §2A).
- [x] `01-017` (M) Import from folder/removable drive: destination naming (date/custom), duplicate skip with "exclude duplicates" checkbox (default on).
- [ ] `01-018` (M) Live search bar wired to FTS (filenames/folders).

## Tasks

- [x] [TASK-01-013-virtualized-grid.md](TASK-01-013-virtualized-grid.md) — build virtualized DOM thumbnail grid with zoom slider
- [ ] [TASK-01-014-canvas-grid.md](TASK-01-014-canvas-grid.md) — build canvas-grid renderer fallback
- [x] [TASK-01-015-folder-tree.md](TASK-01-015-folder-tree.md) — build folder tree with selection and context menu
- [x] [TASK-01-016-folder-manager.md](TASK-01-016-folder-manager.md) — build folder manager dialog
- [ ] [TASK-01-017-import-duplicate-skip.md](TASK-01-017-import-duplicate-skip.md) — build import with destination naming and duplicate skip
- [ ] [TASK-01-018-live-search-bar.md](TASK-01-018-live-search-bar.md) — wire live search bar to FTS
