# ADR-0012: Versioning scheme and release process

**Date:** 2026-10-20 · **Status:** accepted · **Task refs:** TASK-01-003 (signed release builds), TASK-01-026 (perf harness), PLAN §10, §12

## Context

The plan speccs tag-triggered signed release builds (TASK-01-003) and per-release perf runs (TASK-01-026, `docs/webkitgtk-perf-protocol.md`), and ADR-0010 pins distribution to GitHub Releases only — but no version/tag scheme or release procedure is defined anywhere. The pre-first-release checklist in PLAN §12 is partially done (name diligence complete) and needs a durable home.

## Decision

- **Semver tags:** `vMAJOR.MINOR.PATCH`, following the Rust/Cargo and Tauri conventions. The Cargo workspace version and the Tauri app version are bumped together in the release commit.
- **Pre-1.0:** `0.MINOR.PATCH`, where MINOR tracks the plan phase that first ships — e.g. `v0.1.0` = Phase 1 (Viewer & library MVP), `v0.2.0` = Phase 1.5 features, and so on through the phases. Breaking user-visible changes before 1.0 are allowed within a MINOR bump; 1.0 marks the "Picasa successor complete" milestone (end of Phase 4).
- **Tags trigger releases:** pushing a `v*` tag runs the TASK-01-003 pipeline (signed Windows installers, notarized macOS DMG, Linux AppImage/deb/rpm) and publishes a GitHub Release. No other distribution channel (per ADR-0010).
- **Per-release procedure:** the checklist in `docs/workflows/release-checklist.md` is normative for every tagged release.
- **Hotfixes:** `v0.x.y+1` from the release commit, branch `fix/<task-id>-slug` per the usual conventions.

## Consequences

- Version numbers communicate plan progress pre-1.0; after 1.0 they follow standard semver.
- Every release requires the full checklist (including the 50k perf decision run) — releases are deliberate, not opportunistic.
- The updater feed (auto / check-but-ask / off) validates signatures; a release whose artifacts fail signing or the tamper test is not published.
- This ADR does not fix a release cadence; releases happen when a phase's PLAN §6 acceptance criteria pass.
