# feat: job queue & thumbnails

**Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** planned

## Acceptance criteria

- Visible thumbnails are generated before background ones; the queue is bounded and never exhausts memory on a 50k library.
- A job can be cancelled mid-flight without corrupting the on-disk cache; duplicate jobs for the same photo are coalesced.
- Thumbnails served via `thumb://` with zero image bytes crossing IPC; UI never blocks on decode.
- All listed formats (incl. HEIC/AVIF via libheif-rs) produce correct thumbnails; cache is keyed by content hash so re scans don't re-render.

- [ ] `01-009` (M) `jobs` crate: bounded priority queue (visible-first, background-last), throttling hook (battery later), cancellation.
- [ ] `01-010` (L) `render` crate: decode via zune-jpeg/image/libvips bindings; thumbnail pipeline; on-disk cache keyed by content hash.
- [ ] `01-011` (M) `thumb://` Tauri URI scheme handler serving from cache; no image bytes over IPC.
- [ ] `01-012` (M) Format support: JPEG, PNG, GIF, BMP, TIFF, WebP, HEIC, AVIF (libheif-rs); PSD thumbnail if cheap.

## Tasks

- [ ] [TASK-01-009-jobs-queue.md](TASK-01-009-jobs-queue.md) — build jobs crate — bounded priority queue with cancellation
- [ ] [TASK-01-010-render-pipeline.md](TASK-01-010-render-pipeline.md) — build render crate — decode + thumbnail pipeline + cache
- [ ] [TASK-01-011-thumb-uri-scheme.md](TASK-01-011-thumb-uri-scheme.md) — implement thumb:// uri scheme handler
- [ ] [TASK-01-012-format-support.md](TASK-01-012-format-support.md) — support JPEG/PNG/GIF/BMP/TIFF/WebP/HEIC/AVIF decoders
