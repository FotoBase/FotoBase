# feat: UI — viewer & info

**Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** planned

## Acceptance criteria

- Viewer opens from the grid with filmstrip navigation; fullscreen and timed slideshow work with keyboard control.
- Info pane shows EXIF (camera, lens, exposure), a correct histogram, and file properties, updating per photo.
- Locate on disk opens the OS file manager at the photo; Delete moves to OS trash after confirmation; folder removal is catalog-only; Refresh thumbnails re-queues renders.

- [ ] `01-019` (M) Single-photo viewer with filmstrip; fullscreen; basic slideshow (timing, no music).
- [ ] `01-020` (M) Info pane: EXIF (camera, lens, exposure), histogram, file properties.
- [ ] `01-021` (S) File-menu basics: Locate on disk, Delete from disk (trash + confirm), Exit; folder "Remove from Fotobase" (catalog only) + Refresh thumbnails.

## Tasks

- [ ] [TASK-01-019-viewer-filmstrip-slideshow.md](TASK-01-019-viewer-filmstrip-slideshow.md) — build single-photo viewer with filmstrip and slideshow
- [ ] [TASK-01-020-info-pane.md](TASK-01-020-info-pane.md) — build info pane — EXIF, histogram, file properties
- [ ] [TASK-01-021-file-menu-basics.md](TASK-01-021-file-menu-basics.md) — implement file-menu basics and folder commands
