# task: rebuild acceptance harness — delete catalog, rebuild, diff

**Feature:** [FEAT-03-02-os-integration](FEAT-03-02-os-integration.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** todo

## Description
Automated harness proving the P2a acceptance "delete catalog → rebuild → no loss": snapshot user data, wipe the catalog, rebuild from sidecars, diff (PLAN §9 Rebuildability row). This is the regression gate for every sidecar-writing feature in EPIC-03.

## Functional specs
- Harness scripts: snapshot → delete catalog DB → trigger rebuild → diff user data (stars/tags/captions/albums/GPS/stacks/recipes) vs snapshot.
- Diff report is per-field, machine-readable; zero-loss required to pass.
- Runnable on the 50k fixture and on a small fixture set; incremental rebuild path also covered (§5: mtime+size skip).

## Technical specs
- Snapshot = export of catalog user-data tables + sidecar contents hashed; diff tool compares semantically (not byte-diff of DB).
- Rebuild via existing rebuild command; harness drives it headless.
- CI job on the small fixture; the 50k run stays manual/perf-harness (FEAT-01-05) to keep CI fast.
- AI-derived data (faces, embeddings) is out of scope — PLAN §5 allows recompute.

## Testing criteria (TDD)
- Integration: fixture with stars/tags/captions/albums/edits passes zero-loss diff after rebuild.
- Integration: deliberately dropping a sidecar write from one mutation makes the diff FAIL (proves the harness detects regressions).

## Acceptance criteria
- [ ] 03-026: harness reproduces the §9 rebuildability check end-to-end.
- [ ] 03-026: zero user-data loss on the fixture in CI.
- [ ] 03-026: a seeded regression is caught by the harness (verified once, noted in PR).
