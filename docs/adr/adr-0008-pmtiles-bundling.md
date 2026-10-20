# ADR-0008: PMTiles basemap bundling

**Date:** 2026-10-20 · **Status:** accepted (PLAN §11 Q7)

## Context

The map must work fully offline, but the installer goal is < 20 MB.

## Decision

Ship a compact low-zoom world basemap (~5–10 MB) in the installer; high-zoom regional PMTiles are opt-in downloads with explicit user consent. No silent network fetches, ever.

## Consequences

Map works out of the box offline at world scale; detail seekers download regions once; the no-network promise is test-enforced (06-002 acceptance).
