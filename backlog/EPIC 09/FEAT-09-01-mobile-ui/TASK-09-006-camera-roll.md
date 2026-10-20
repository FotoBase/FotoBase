# task: add camera roll as watched-folder equivalent

**Feature:** [FEAT-09-01-mobile-ui](FEAT-09-01-mobile-ui.md) · **Epic:** [EPIC-09 — Mobile](../EPIC-09.md) · **Status:** todo

## Description
Expose the device camera roll as a catalog source via platform photo APIs (PLAN §6 Phase 5: camera roll as a "watched-folder" equivalent). Reuses the EPIC-01 catalog scanning model; observation must be battery-friendly.

## Functional specs
- Camera roll photos appear in the app without manual import.
- New photos appear without relaunching the app.
- Observation does not drain battery: event-driven, not polling.

## Technical specs
- Use platform photo/change-notification APIs (iOS PHPhotoLibrary change observer, Android MediaStore ContentObserver) bridged through the Tauri mobile shell.
- Feed events into the existing catalog scan/watch pipeline from EPIC-01; no parallel scanner.

## Testing criteria (TDD)
- integration (emulator): adding a photo to the camera roll surfaces it in the catalog.
- unit: change events map to the same incremental-scan requests as folder watcher events.

## Acceptance criteria
- [ ] 09-006: Camera roll photos are visible in the app via platform photo APIs.
- [ ] New/changed photos are picked up without app restart.
- [ ] Observation is event-driven (no polling loop).
