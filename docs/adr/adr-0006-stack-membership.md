# ADR-0006: Stack membership lives in sidecars

**Date:** 2026-10-20 · **Status:** accepted (PLAN §11 Q5)

## Context

RAW+JPEG pairs, bursts, and edited versions form stacks; membership must survive catalog rebuild and ideally travel with synced copies.

## Decision

`fb:StackId` + `fb:StackRole` fields on each member's sidecar; the catalog holds only a derived stack index.

## Consequences

Stacks survive rebuild and Syncthing copies; a stack op writes N sidecars (accepted cost); format defined normatively in docs/sidecar-merge-on-write.md §2.
