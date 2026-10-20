# feat: app skeleton & CI

**Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** planned

## Acceptance criteria

- `cargo build` + `npm run build` succeed from a clean clone on all three OSes per §3 module layout.
- CI matrix (windows-latest, macos-latest arm64 + x64, ubuntu-latest) builds and runs tests green on every PR.
- Release artifacts for all three OSes are produced by the Tauri bundler, signed (MSI/NSIS, notarized DMG, AppImage/deb/rpm), and the updater plugin verifies signatures.
- WebKitGTK benchmark run recorded in an ADR with the DOM-vs-canvas decision and its numeric justification.

- [ ] `01-001` (M) Cargo workspace + Tauri v2 + Svelte 5 + Vite skeleton per §3 module layout (crates: core, catalog, scanner, metadata, render, jobs first).
- [ ] `01-002` (M) GitHub Actions matrix: windows-latest, macos-latest (arm64 + x64), ubuntu-latest; build + test on all.
- [ ] `01-003` (M) Signed release builds for 3 OSes via Tauri bundler (MSI/NSIS, DMG notarized, AppImage/deb/rpm) + signed updater plugin wiring.
- [ ] `01-004` (S) Run WebKitGTK benchmark (00-012 protocol); decide DOM vs canvas grid as Linux default; record ADR.

## Tasks

- [ ] [TASK-01-001-workspace-skeleton.md](TASK-01-001-workspace-skeleton.md) — scaffold cargo workspace + tauri + svelte app
- [ ] [TASK-01-002-ci-matrix.md](TASK-01-002-ci-matrix.md) — set up github actions CI matrix
- [ ] [TASK-01-003-signed-release-builds.md](TASK-01-003-signed-release-builds.md) — produce signed release builds with updater wiring
- [ ] [TASK-01-004-grid-decision-adr.md](TASK-01-004-grid-decision-adr.md) — run webkitgtk benchmark and decide dom vs canvas
