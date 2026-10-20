# feat: performance fixture

**Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** planned

## Acceptance criteria

- Generator script (committed, no data) produces a 50k-photo tree: mixed formats (JPEG/PNG/WebP/HEIC/TIFF), spread dates, varied folder depth, deterministic given a seed.
- Generation runs in CI in minutes, not hours; size is configurable (1k smoke / 50k full).
- Every epic's perf-related acceptance tests can consume the fixture via a documented one-liner.

- [x] `00-013` (M) Build the 50k-photo test fixture generator (mixed formats, dates, folder depth; scriptable, committed as generator not data). Used by every epic's acceptance tests (§9). Done 2026-10-20; determinism verified.

## Tasks

- [x] [TASK-00-013-fixture-generator.md](TASK-00-013-fixture-generator.md) — build the 50k-photo fixture generator
