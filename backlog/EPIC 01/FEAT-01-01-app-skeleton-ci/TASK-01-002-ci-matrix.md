# task: set up github actions CI matrix

**Feature:** [FEAT-01-01-app-skeleton-ci](FEAT-01-01-app-skeleton-ci.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** todo

## Description

GitHub Actions matrix per PLAN §4: windows-latest, macos-latest (arm64 + x64), ubuntu-latest — build and test on every PR, honoring the 00-003 OS floors.

## Functional specs

- Every PR runs build + tests on all matrix legs; red CI blocks merge.

## Technical specs

- Implemented as `.github/workflows/ci.yml` (the docs-only `docs-lint.yml` and `fixture-smoke.yml` already exist; this task adds the code CI).
- Matrix includes macOS arm64 and x64 targets; cargo test + frontend build (Vitest once FEAT-01-06 lands).
- Caching (cargo, node) to keep runs fast.
- `cargo audit` for Rust dependency advisories (per `docs/workflows/dependency-tracking.md`). Also add a `cargo-audit.yml` scheduled workflow (weekly cron) once the workspace exists.

## Testing criteria (TDD)

- Meta: a deliberately broken commit on a PR fails the matrix; revert restores green.

## Acceptance criteria

- [ ] 01-002: CI matrix (windows-latest, macos-latest arm64+x64, ubuntu-latest) builds + tests green on every PR.
