# ADR-0002: Star vs 1–5 ratings

**Date:** 2026-10-20 · **Status:** accepted (PLAN §11 Q1)

## Context

Picasa used a single star; other tools use 1–5 ratings. Both must map to one interoperable field.

## Decision

Single star by default (maps to `xmp:Rating` = 5); 1–5 ratings as an opt-in setting. Both write the same field.

## Consequences

Simpler default UX; ratings available for power users; migration from Picasa is 1:1.
