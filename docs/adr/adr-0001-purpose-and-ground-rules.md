# ADR-0001: Purpose and ground rules

**Date:** 2026-10-20 · **Status:** accepted

## Context

Fotobase needs a durable, searchable record of product and architecture decisions. PLAN.md §11 holds open questions and their resolutions, but decisions deserve stable, linkable artifacts.

## Decision

Decisions are recorded as lightweight ADRs in `docs/adr/`, numbered `adr-NNNN-slug.md`, with the format used here: **Date · Status · Context · Decision · Consequences**. PLAN §11 remains the index of open questions; once resolved, a decision gets an ADR and PLAN strikes the question through.

## Consequences

- The 2026-10-20 founding decisions are seeded as ADR-0002…ADR-0009.
- Any PR that contradicts an accepted ADR must first supersede it with a new ADR.
