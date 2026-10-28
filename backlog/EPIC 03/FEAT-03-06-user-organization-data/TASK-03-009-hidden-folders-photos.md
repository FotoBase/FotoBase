# task: hidden folders and hidden photos

**Feature:** [FEAT-03-06-user-organization-data](FEAT-03-06-user-organization-data.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in progress (2026-10-28)

## Description
Hide folders and individual photos from the library view only — files on disk untouched (PLAN §2 A Hidden folders, §6 P2a). Hide actions route through the catalog trash (03-012) for undo.

## Functional specs
- Hide a folder: it disappears from folder tree and grid; unhide restores it.
- Hide a photo: disappears from grid/search; unhide restores.
- Hidden items excluded from search results and filters; a "show hidden" toggle in Options reveals them grayed.
- Files on disk are never moved, renamed, or modified.

## Technical specs
- Hidden flag is rebuildable user data (PLAN §11 Q6, decided 2026-10-20): photo hide → `fb:Hidden` in the photo's sidecar; folder hide → flag in a folder-level `.fotobase` metadata sidecar, member photos inherit.
- Hide/unhide through the trash soft-delete path (03-012) for the undo window.
- All query paths (grid, FTS search, filters) filter on the flag.

## Testing criteria (TDD)
- Unit: hidden photo excluded from a search query that would otherwise match it; "show hidden" includes it.
- Integration: hide → rebuild → still hidden; unhide restores with stars/tags intact.

## Acceptance criteria
- [x] 03-009: folders and photos can be hidden/unhidden, library-only.
- [x] 03-009: hidden items excluded from search and filters; toggle reveals them.
- [x] 03-009: hide is undoable (trash) and rebuildable; disk untouched.
