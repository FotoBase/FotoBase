# feat: platform

**Epic:** [EPIC-09 — Mobile](../EPIC-09.md) · **Status:** planned

## Acceptance criteria

- iOS and Android build targets compile the same Rust core; CI lanes build both on PR.
- Read-only-safe catalog open against a NAS or Syncthing/iCloud-synced folder: concurrent external writes never corrupt state; app never writes to the source unguarded.

- [ ] `09-001` (L) Tauri v2 iOS/Android build targets sharing the Rust core; CI lanes.
- [ ] `09-002` (M) Catalog source: single-catalog access (NAS or Syncthing/iCloud-synced folder); read-only-safe open path.

## Tasks

- [ ] [TASK-09-001-tauri-mobile-builds.md](TASK-09-001-tauri-mobile-builds.md) — set up tauri v2 ios/android targets and ci lanes
- [ ] [TASK-09-002-catalog-source-readonly.md](TASK-09-002-catalog-source-readonly.md) — implement read-only-safe single-catalog source
