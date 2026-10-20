# ADR-0003: Save writes sidecars only by default

**Date:** 2026-10-20 · **Status:** accepted (PLAN §11 Q2)

## Context

"Save" could embed XMP into original files (as Picasa did) or keep all user data in sidecars.

## Decision

Sidecars only by default; opt-in setting to embed XMP into the original on Save.

## Consequences

Originals stay untouched (safest default, aligns with the read-only rule); interop tools still read the sidecars; embed is a deliberate user action.
