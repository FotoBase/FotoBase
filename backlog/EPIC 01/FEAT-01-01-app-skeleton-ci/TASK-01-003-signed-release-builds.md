# task: produce signed release builds with updater wiring

**Feature:** [FEAT-01-01-app-skeleton-ci](FEAT-01-01-app-skeleton-ci.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in progress (2026-10-21)

## Description

Release packaging per PLAN §4 (Tauri bundler): MSI/NSIS on Windows, notarized DMG on macOS, AppImage/deb/rpm on Linux, plus the signed Tauri updater plugin so updates verify signatures.

## Functional specs

- Tagged releases produce installers for all three OSes; installers run on the 00-003 OS floors.
- Updater: app checks for updates and only installs artifacts with valid signatures.

## Technical specs

- Implemented as `.github/workflows/release.yml`, triggered by `v*` tags (versioning per ADR-0012); steps follow `docs/workflows/release-checklist.md`.
- Tauri v2 bundler + updater plugin (signed, §4); macOS notarization via CI secrets; Windows signing per Tauri docs.
- Secrets (signing keys, Apple credentials) held in GitHub Actions secrets, never in repo.

## Testing criteria (TDD)

- Release smoke: install each artifact on its OS, launch, verify the updater rejects a tampered signature in a test.

## Acceptance criteria

- [x] 01-003: Signed MSI/NSIS, notarized DMG, AppImage/deb/rpm artifacts produced by CI; signed updater wired.
