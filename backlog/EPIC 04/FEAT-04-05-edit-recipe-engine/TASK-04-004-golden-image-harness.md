# task: build golden-image test harness for recipe ops

**Feature:** [FEAT-04-05-edit-recipe-engine](FEAT-04-05-edit-recipe-engine.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** in review (2026-10-30)

## Description
A golden-image regression harness (per-pixel tolerance, PLAN §9 Render fidelity) that every recipe op must pass, proving preview and export paths produce identical results (epic acceptance).

## Functional specs
- Every recipe op has at least one golden-image test recording expected output.
- Failures show per-pixel diffs, not just pass/fail.

## Technical specs
- Rust test harness in `crates/render` (or a test-support crate): render op on fixture → compare against committed golden image with per-pixel tolerance (§9).
- Fixtures checked in; tolerances defined per op.
- Runs in CI on all three OSes (§4 CI matrix); complements `insta` snapshots (§4 Testing).

## Testing criteria (TDD)
- Meta-test: harness detects an intentionally perturbed op output as a failure.
- Integration: each op's preview-res and full-res outputs match their goldens within tolerance.

## Acceptance criteria
- [x] 04-004: Golden-image harness exists with per-pixel tolerance and covers every recipe op.
- [x] 04-004: Harness wired into CI and fails on regressions.
