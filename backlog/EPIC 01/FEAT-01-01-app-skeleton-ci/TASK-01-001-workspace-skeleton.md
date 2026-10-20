# task: scaffold cargo workspace + tauri + svelte app

**Feature:** [FEAT-01-01-app-skeleton-ci](FEAT-01-01-app-skeleton-ci.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** todo

## Description
Create the application skeleton per PLAN §3 module layout: a Cargo workspace with the first crates (core, catalog, scanner, metadata, render, jobs), a Tauri v2 shell, and a Svelte 5 + TypeScript + Vite frontend that builds cleanly from a fresh clone.

## Functional specs
- App launches to an empty window on all three OSes.
- `cargo build` + `npm run build` succeed from a clean clone.

## Technical specs
- Workspace per §3: `crates/{core,catalog,scanner,metadata,render,jobs}`, `app/src-tauri/` (Tauri v2 shell + commands), `app/src/` (Svelte 5 + TS).
- Vite build; plain CSS / CSS variables (no CSS framework, §4); `tracing`, `thiserror`/`anyhow` for logging/errors.

## Testing criteria (TDD)
- CI: clean-clone build on windows-latest, macos-latest (arm64+x64), ubuntu-latest.
- Unit: each crate has a trivially passing `cargo test` target so the matrix validates from day one.

## Acceptance criteria
- [ ] 01-001: Workspace + Tauri v2 + Svelte 5 + Vite skeleton per §3 layout; builds clean on all three OSes.
