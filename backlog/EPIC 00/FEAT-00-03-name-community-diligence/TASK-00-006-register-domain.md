# task: check and register the project domain

**Feature:** [FEAT-00-03-name-community-diligence](FEAT-00-03-name-community-diligence.md) · **Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** deferred (availability checked 2026-10-20 per [ADR-0010](../../../docs/adr/adr-0010-name-diligence-sweep.md); register when a website exists)

## Description
Per PLAN §12: check availability of `fotobase.app` / `fotobase.dev` / `fotobase.io`, register the chosen domain, and park it. Blocking before first public release.

## Functional specs
- Domain registered and parked; whois/registry details stored in the project's private notes.

## Technical specs
- Preferred order per PLAN §12: `.app` / `.dev` / `.io`.

## Testing criteria (TDD)
- Checklist: chosen domain resolves (or is parked) and registry details are recorded.

## Acceptance criteria
- [x] 00-006 (partial): availability checked for all three TLDs (ADR-0010: `.app` taken; `.dev`, `.io` free).
- [ ] 00-006 (deferred): register chosen domain (`fotobase.dev`) and park it — when a website exists.
