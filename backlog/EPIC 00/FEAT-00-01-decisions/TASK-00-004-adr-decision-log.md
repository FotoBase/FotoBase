# task: set up ADR-style decision log and seed it

**Feature:** [FEAT-00-01-decisions](FEAT-00-01-decisions.md) · **Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** done (2026-10-20) → [`docs/adr/`](../../../docs/adr/)

## Description
Establish the decision-log format (ADR-style) in `docs/adr/` and seed it with the four §11 decisions already resolved in PLAN.md (rating model, sidecar-vs-embed, project name, OS floors) so plan and log never drift.

## Functional specs
- Each ADR records date, status, context, and consequences.
- PLAN.md §11 links to the corresponding ADRs.
- A short template file lets future decisions follow the same format without asking how.

## Technical specs
- `docs/adr/` with numbered ADR files (e.g. `0001-single-star-rating.md`) plus `TEMPLATE.md`.
- Markdown only; linked from PLAN §11.

## Testing criteria (TDD)
- Docs check (CI or lint): every §11 resolved question links to an existing ADR file; template exists.

## Acceptance criteria
- [x] 00-004: `docs/adr/` exists with ADR template + seeded ADRs for 00-001..00-003 (and the name decision from §11 Q3). Seeded through Q8: adr-0001…0009.
- [ ] PLAN.md §11 links each decision to its ADR.
- [ ] New decisions can be written up from the template alone.
