# task: set up tauri v2 ios/android targets and ci lanes

**Feature:** [FEAT-09-02-platform](FEAT-09-02-platform.md) · **Epic:** [EPIC-09 — Mobile](../EPIC-09.md) · **Status:** todo

## Description
Stand up iOS and Android build targets on Tauri v2 sharing the same Rust core as desktop, plus CI lanes building both on PR (PLAN §6 Phase 5). This is the platform foundation for all of EPIC-09.

## Functional specs
- The app compiles and launches on iOS and Android from one codebase.
- No feature divergence required in the Rust core between desktop and mobile.

## Technical specs
- Tauri v2 mobile targets; Rust core compiled unchanged (or behind minimal cfg gates) for both.
- CI lanes (GitHub Actions matrix: macOS runner for iOS, Linux/Android for Android) build both on PR; mobile lanes may be allowed-to-fail initially if flaky, then promoted.

## Testing criteria (TDD)
- CI: a PR that breaks the mobile build fails the mobile lane (verified by an intentional break once).
- e2e smoke: app launches to an empty/grid state on both simulators/emulators in CI.

## Acceptance criteria
- [ ] 09-001: iOS and Android targets compile the same Rust core.
- [ ] CI lanes build both platforms on every PR.
- [ ] App launches on both iOS and Android.
