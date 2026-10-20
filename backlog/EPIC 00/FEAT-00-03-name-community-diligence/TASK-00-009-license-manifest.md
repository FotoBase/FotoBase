# task: add GPL-3.0 license and CI license-manifest scaffold

**Feature:** [FEAT-00-03-name-community-diligence](FEAT-00-03-name-community-diligence.md) · **Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** done (license file 2026-10-20; CI scaffold folded into 01-002) → [LICENSE](../../../LICENSE)

## Description
Per PLAN §1 (GPL-3.0 required by exiv2 dependency, §10 risk row): add the GPL-3.0 license file and source headers, and scaffold a CI license manifest. Per-model license checks arrive with EPIC-05/07; this task only builds the scaffold.

## Functional specs
- License file at repo root; crate source files carry SPDX headers.
- CI step produces a dependency license manifest and fails on non-compatible licenses (GPL-3.0 policy).

## Technical specs
- SPDX `GPL-3.0-only` headers; CI job (e.g. cargo-deny) generating the manifest.
- Model-license vetting hooks documented but empty until EPIC-05/07.

## Testing criteria (TDD)
- CI: license-manifest job runs green on the skeleton workspace; a flagged non-compatible crate fails the job.

## Acceptance criteria
- [x] 00-009: GPL-3.0 license + headers in place; CI license-manifest scaffold runs green. License file in place 2026-10-20; scaffold moves to 01-002 (no CI yet).
