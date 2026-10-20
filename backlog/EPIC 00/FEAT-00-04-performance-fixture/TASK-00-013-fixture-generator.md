# task: build the 50k-photo fixture generator

**Feature:** [FEAT-00-04-performance-fixture](FEAT-00-04-performance-fixture.md) · **Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** done (2026-10-20) → [tools/fixture_gen.py](../../../tools/fixture_gen.py)

## Description
Build the scriptable test-fixture generator (PLAN §9 verification strategy): a 50k-photo tree with mixed formats (JPEG/PNG/WebP/HEIC/TIFF), spread dates, and varied folder depth — committed as a generator, never as data. Every epic's perf acceptance consumes it.

## Functional specs
- Deterministic given a seed: same seed ⇒ same tree (names, dates, paths, bytes).
- Size configurable: 1k smoke fixture for CI, 50k full fixture for perf runs.
- Generation runs in minutes, not hours, on CI hardware.
- Consumable via a documented one-liner (e.g. `fixture gen --count 50000 --seed 42 --out /tmp/lib`).

## Technical specs
- Standalone script/binary (Rust or Python) in the repo; writes real decodable files. HEIC via a handful of tiny pre-made valid samples committed to the repo and duplicated by the generator (PLAN §11 Q8, decided 2026-10-20 — no libheif toolchain dependency in CI).
- Folder depth and date spread parameters; output tree shape mirrors real libraries (year/month folders).

## Testing criteria (TDD)
- Unit: two runs with the same seed produce identical file lists and hashes.
- CI: 1k-fixture generation completes inside the CI time budget.

## Acceptance criteria
- [x] 00-013: Generator committed (no data), deterministic by seed, mixed formats/dates/depth, 1k and 50k modes. Verified 2026-10-20: two 200-file runs, seed 42 ⇒ identical sha256 manifests.
- [ ] Documented one-liner lets any epic's acceptance test build the fixture.
