# Release checklist

**Governs:** every tagged release (see ADR-0012) · **Plan refs:** §10, §12 · **Related:** `docs/webkitgtk-perf-protocol.md`, TASK-01-003, TASK-01-026

Steps in order. A release is not published if any step fails.

## 1. Pre-tag

- [ ] All tasks for the shipped scope are checked off in the backlog (both epic and FEAT files).
- [ ] PLAN §6 acceptance criteria for the phase pass.
- [ ] CI matrix (windows-latest, macos-latest arm64+x64, ubuntu-latest) is green: `cargo test`, clippy, frontend build + Vitest, 1k perf smoke produces a schema-valid `perf-report.json`.
- [ ] Model license manifest check passes (once AI models ship, EPIC-05/07).
- [ ] Version bumped in the Cargo workspace and Tauri app (together, same number).
- [ ] Dependency watch (`docs/workflows/dependency-tracking.md`) reviewed — no untriaged upstream security releases.

## 2. Perf decision run

- [ ] Full 50k-fixture run on the recorded reference machine (oldest-supported Ubuntu 22.04 hardware), per `docs/webkitgtk-perf-protocol.md`.
- [ ] `perf-report.json` produced, committed for trend comparison, and all bars pass (first thumbnail < 2 s; ≥ 60 fps; < 150 MB idle; 0 jank events).
- [ ] If any bar is missed: canvas fallback becomes the Linux default, recorded in an ADR — do not ship the DOM default on Linux over a failed bar.

## 3. Build & verify artifacts

- [ ] Tag `v0.x.y` pushed; TASK-01-003 pipeline builds signed Windows (MSI/NSIS), notarized macOS DMG, Linux AppImage/deb/rpm.
- [ ] Installer size < 20 MB per artifact.
- [ ] Install smoke test on each OS: app launches, scans the 1k fixture, first thumbnails render.
- [ ] Updater test: signed update accepted; tampered artifact rejected.

## 4. Publish

- [ ] GitHub Release created (GitHub Releases only — ADR-0010) with notes listing plan-phase scope and perf numbers.
- [ ] Post-release: backlog checkboxes and any perf-related ADRs updated (see the `backlog-maintenance` skill).

## Pre-first-release only (PLAN §12)

- [ ] Name diligence done (2026-10-20 — org `FotoBase`, ADR-0010/0011).
- [ ] Trademark sweep clean (ADR-0011).
- [ ] crates.io/npm claims dropped (ADR-0010).
