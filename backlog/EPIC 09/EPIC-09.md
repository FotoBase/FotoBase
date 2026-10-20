# EPIC-09 — Mobile

**Plan ref:** §6 Phase 5 · **Size:** M · **Depends on:** EPIC-01 core; decoupled from F1.2 (ships against a single catalog source: NAS or synced folder)

## Acceptance

- Read-mostly mobile app builds from the same Rust core; browse/search/view/star/tag/share on iOS + Android.

## feat: platform

- [ ] `09-001` (L) Tauri v2 iOS/Android build targets sharing the Rust core; CI lanes.
- [ ] `09-002` (M) Catalog source: single-catalog access (NAS or Syncthing/iCloud-synced folder); read-only-safe open path.

## feat: mobile UI

- [ ] `09-003` (M) Browse + grid + viewer adapted for touch (Svelte reuse, mobile layout).
- [ ] `09-004` (M) Search (FTS) + star/tag editing on device.
- [ ] `09-005` (M) Share sheet integration.
- [ ] `09-006` (M) Camera roll as watched-folder equivalent via platform photo APIs.
