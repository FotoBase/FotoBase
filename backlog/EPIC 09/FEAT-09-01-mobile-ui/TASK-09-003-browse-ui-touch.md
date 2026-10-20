# task: build touch-adapted browse, grid, and viewer ui

**Feature:** [FEAT-09-01-mobile-ui](FEAT-09-01-mobile-ui.md) · **Epic:** [EPIC-09 — Mobile](../EPIC-09.md) · **Status:** todo

## Description
Adapt the existing Svelte browse/grid/viewer UI for touch on iOS and Android (PLAN §6 Phase 5: read-mostly first). Reuses desktop components from EPIC-01/EPIC-07 rather than a separate mobile app UI. Depends on FEAT-09-02 (Tauri mobile targets) to run.

## Functional specs
- Browse, grid, and viewer flows are usable with touch only: tap to open, swipe to navigate, pinch to zoom.
- Grid layout adapts to phone portrait/landscape sizes with sensible thumbnail density.
- Viewer supports swipe between photos and pinch-zoom; no hover-dependent interactions.
- App remains read-mostly per PLAN: no editing beyond what later tasks (09-004) add.

## Technical specs
- Reuse Svelte components from the EPIC-01 UI library; add mobile layout variants, not a fork.
- Tauri v2 mobile webview; no platform-specific UI frameworks.
- Touch gestures via pointer events / CSS where possible over JS gesture libraries.

## Testing criteria (TDD)
- e2e (mobile webview or Playwright touch emulation): tap-to-open viewer, swipe-to-navigate, pinch-zoom behave correctly.
- unit: grid layout produces expected column counts at defined viewport breakpoints.

## Acceptance criteria
- [ ] 09-003: Browse, grid, and viewer work on-device with touch gestures (tap, swipe, pinch).
- [ ] Grid and viewer render correctly in phone portrait and landscape.
- [ ] UI reuses the existing Svelte component set (no separate mobile component tree).
