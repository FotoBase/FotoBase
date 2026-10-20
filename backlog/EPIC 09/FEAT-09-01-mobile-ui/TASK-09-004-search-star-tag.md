# task: add on-device search and star/tag editing

**Feature:** [FEAT-09-01-mobile-ui](FEAT-09-01-mobile-ui.md) · **Epic:** [EPIC-09 — Mobile](../EPIC-09.md) · **Status:** todo

## Description
Bring search (FTS) and star/tag editing to the mobile app (PLAN §6 Phase 5: "browse, search, view, star, tag"). Runs on the shared Rust core catalog; writes must respect the read-only-safe catalog source from FEAT-09-02.

## Functional specs
- Search box with FTS over the catalog returns results on-device, same ranking as desktop.
- Star rating can be set/changed from grid and viewer.
- Tags can be added/removed on a photo from the viewer.
- Edits persist to the catalog without corrupting it when the source is externally written (NAS/synced folder).

## Technical specs
- Reuse the desktop FTS query layer from the Rust core; no separate mobile search implementation.
- Star/tag writes go through the same catalog write path as desktop (XMP/sidecar layer per EPIC-03 where applicable).

## Testing criteria (TDD)
- integration: FTS query on a fixture catalog returns expected photos on mobile build.
- integration: setting a star / adding a tag persists and survives app restart.
- unit: write path is a no-op or safe-fails when the catalog source is read-only.

## Acceptance criteria
- [ ] 09-004: Search (FTS), star, and tag editing work on-device.
- [ ] Star/tag changes persist across app restarts.
- [ ] Writes are safe against a read-only or externally-synced catalog source.
