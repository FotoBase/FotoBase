# task: search trademark registries

**Feature:** [FEAT-00-03-name-community-diligence](FEAT-00-03-name-community-diligence.md) · **Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** done (2026-10-20, clean → [ADR-0011](../../../docs/adr/adr-0011-trademark-search.md))

## Description
Per PLAN §12: search EUIPO, USPTO, and DPMA trademark registries for "fotobase" (minimum), record results in an ADR, and accept that the descriptive name has weak protectability — the moat is the product. Any risk found triggers a rename before first public commit.

## Functional specs
- EUIPO, USPTO, DPMA searched at minimum; result documented in an ADR.
- If risk is found, a rename decision is taken before first public commit.

## Technical specs
- Findings in `docs/adr/` via the 00-004 ADR format.

## Testing criteria (TDD)
- Checklist: ADR lists each registry searched, date, and findings.

## Acceptance criteria
- [x] 00-007: EUIPO, USPTO, DPMA searched; result recorded in ADR; go/no-go on the name documented. Go — ADR-0011.
