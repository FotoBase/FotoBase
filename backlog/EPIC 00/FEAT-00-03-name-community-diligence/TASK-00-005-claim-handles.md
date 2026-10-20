# task: check name collisions and claim handles

**Feature:** [FEAT-00-03-name-community-diligence](FEAT-00-03-name-community-diligence.md) · **Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** done (descoped 2026-10-20, [ADR-0010](../../../docs/adr/adr-0010-name-diligence-sweep.md))

## Description
Per PLAN §12 pre-first-release checklist: sweep GitHub org, crates.io, and npm for "fotobase" collisions, claim the handles, and record the result in an ADR. Must complete before the first public commit.

## Functional specs
- No blocking collisions on GitHub org, crates.io, npm; handles claimed.
- Result recorded in an ADR; a collision triggers a rename decision before first public commit.

## Technical specs
- Manual registry checks; findings go in `docs/adr/` (00-004 format).

## Testing criteria (TDD)
- Checklist review: ADR exists naming each registry checked, date, and claim status.

## Acceptance criteria
- [x] 00-005: GitHub org, crates.io, npm checked for "fotobase"; handles claimed; ADR records the sweep. Descoped: all three verified free (ADR-0010); crates.io/npm claims dropped (GitHub Releases-only distribution); GitHub name claimed by creating the `fotobase` repo at first public commit.
