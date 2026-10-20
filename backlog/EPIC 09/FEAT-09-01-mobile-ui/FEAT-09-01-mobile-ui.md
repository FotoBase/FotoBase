# feat: mobile UI

**Epic:** [EPIC-09 — Mobile](../EPIC-09.md) · **Status:** planned

## Acceptance criteria

- Read-mostly flows work on-device: browse, grid, viewer, search (FTS), star, tag, share (epic acceptance).
- Touch-adapted UI reuses Svelte components; platform share sheet targets work for images.
- Camera roll appears via platform photo APIs as a watched-folder equivalent; battery-friendly observation.

- [ ] `09-003` (M) Browse + grid + viewer adapted for touch (Svelte reuse, mobile layout).
- [ ] `09-004` (M) Search (FTS) + star/tag editing on device.
- [ ] `09-005` (M) Share sheet integration.
- [ ] `09-006` (M) Camera roll as watched-folder equivalent via platform photo APIs.

## Tasks

- [ ] [TASK-09-003-browse-ui-touch.md](TASK-09-003-browse-ui-touch.md) — build touch-adapted browse, grid, and viewer ui
- [ ] [TASK-09-004-search-star-tag.md](TASK-09-004-search-star-tag.md) — add on-device search and star/tag editing
- [ ] [TASK-09-005-share-sheet.md](TASK-09-005-share-sheet.md) — integrate platform share sheet
- [ ] [TASK-09-006-camera-roll.md](TASK-09-006-camera-roll.md) — add camera roll as watched-folder equivalent
