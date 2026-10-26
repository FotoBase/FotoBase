# task: golden-image tests for the display transform

**Feature:** [FEAT-02-01-color-management](FEAT-02-01-color-management.md) · **Epic:** [EPIC-02 — Media & platform hardening](../EPIC-02.md) · **Status:** in review (2026-10-26)

## Description
Add golden-image tests covering sRGB, Display-P3, and Adobe RGB sources through the display transform (PLAN §9 "Render fidelity: golden images per recipe op"), plus a documented visual-comparison protocol against OS viewers. Guards the 02-001 implementation against regressions.

## Functional specs
- CI passes golden-image comparison for all three source profiles.
- Visual comparison vs OS viewers is repeatable (fixed fixtures, documented steps, expected deltas).

## Technical specs
- Store reference PNGs in the test fixtures; compare with a perceptual tolerance (e.g. RMS delta under threshold), not byte equality — lcms2 versions vary.
- Fixtures: one synthetic image per source profile with saturated primaries (max transform sensitivity).
- Comparison runner as a plain `cargo test` integration test in the `render` crate; no external service.

## Testing criteria (TDD)
- Unit: a deliberately broken transform (identity instead of ICC) fails the golden test — proves the test detects regressions.
- Integration: full matrix (3 source profiles × sRGB + P3 display profiles) runs in CI in seconds (small fixtures).

## Acceptance criteria
- [x] 02-002: golden-image tests for sRGB, Display-P3, Adobe RGB pass in CI.
- [x] 02-002: a mutated transform is caught by the test (verified once, noted in PR).
- [x] 02-002: visual-comparison protocol vs OS viewers is documented in the repo.
