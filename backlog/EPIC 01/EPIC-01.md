# EPIC-01 — Viewer & library MVP

**Plan ref:** §6 Phase 1 · **Size:** L · **Depends on:** EPIC-00 (00-012 benchmark gate)

## Acceptance (from PLAN §6 P1)

- 50k photos: first thumbnails visible < 2 s after scan start; full scan completes in background without UI jank.
- Idle RAM < 150 MB; scrolling at 60 fps on all three OSes (DOM grid or canvas fallback, whichever passes).

## feat: app skeleton & CI

- [x] `01-001` (M) Cargo workspace + Tauri v2 + Svelte 5 + Vite skeleton per §3 module layout (crates: core, catalog, scanner, metadata, render, jobs first).
- [x] `01-002` (M) GitHub Actions matrix: windows-latest, macos-latest (arm64 + x64), ubuntu-latest; build + test on all.
- [x] `01-003` (M) Signed release builds for 3 OSes via Tauri bundler (MSI/NSIS, DMG notarized, AppImage/deb/rpm) + signed updater plugin wiring.
- [ ] `01-004` (S) Run WebKitGTK benchmark (00-012 protocol); decide DOM vs canvas grid as Linux default; record ADR.

## feat: catalog & scanning

- [x] `01-005` (M) `catalog` crate: SQLite schema (§5 tables minus AI ones), migrations, WAL, startup `integrity_check`, rolling backups.
- [x] `01-006` (M) `scanner` crate: folder walk, mtime+size change detection, initial scan populating catalog; incremental by default.
- [x] `01-007` (M) `notify` watcher for "Scan Always" folders; watcher events debounced into the job queue.
- [x] `01-008` (M) FTS5 over filename + folder name; live search query API.

## feat: job queue & thumbnails

- [x] `01-009` (M) `jobs` crate: bounded priority queue (visible-first, background-last), throttling hook (battery later), cancellation.
- [x] `01-010` (L) `render` crate: decode via zune-jpeg/image/libvips bindings; thumbnail pipeline; on-disk cache keyed by content hash.
- [x] `01-011` (M) `thumb://` Tauri URI scheme handler serving from cache; no image bytes over IPC.
- [x] `01-012` (M) Format support: JPEG, PNG, GIF, BMP, TIFF, WebP, HEIC, AVIF (libheif-rs); PSD thumbnail if cheap.

## feat: UI — library

- [x] `01-013` (M) Virtualized thumbnail grid (TanStack Virtual) with zoom slider; DOM implementation.
- [x] `01-014` (M) Canvas-grid renderer (Linux default if 01-004 says so; fallback everywhere).
- [x] `01-015` (M) Folder tree grouped by year; selection state; context menu skeleton.
- [x] `01-016` (M) Folder Manager dialog: Scan Once / Scan Always / Remove from catalog (per §2A).
- [x] `01-017` (M) Import from folder/removable drive: destination naming (date/custom), duplicate skip with "exclude duplicates" checkbox (default on).
- [x] `01-018` (M) Live search bar wired to FTS (filenames/folders).

## feat: UI — viewer & info

- [x] `01-019` (M) Single-photo viewer with filmstrip; fullscreen; basic slideshow (timing, no music).
- [x] `01-020` (M) Info pane: EXIF (camera, lens, exposure), histogram, file properties.
- [x] `01-021` (S) File-menu basics: Locate on disk, Delete from disk (trash + confirm), Exit; folder "Remove from Fotobase" (catalog only) + Refresh thumbnails.

## feat: options & platform polish

- [x] `01-022` (M) Options dialog (General): language, automatic updates (auto/check-but-ask/off), clear cache files, show tooltips.
- [x] `01-023` (M) Rebuild catalog command (from watched folders; progress-reported).
- [ ] `01-024` (M) Dark mode + HiDPI on all three OSes; keyboard shortcuts for core flows.
- [ ] `01-025` (S) File-association option: register as default viewer for image types (P1 scope: registration only, viewer exists via 01-019).

## feat: perf acceptance harness

- [ ] `01-026` (M) Perf harness on 00-013 fixture: time-to-first-thumb, idle/peak RAM, scroll fps per OS; runs in CI as smoke test (per §10 WebView-drift risk).
