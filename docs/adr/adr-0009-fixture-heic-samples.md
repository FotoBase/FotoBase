# ADR-0009: Fixture HEIC via committed samples

**Date:** 2026-10-20 · **Status:** accepted (PLAN §11 Q8)

## Context

The 50k perf fixture (00-013) needs HEIC files, but CI must not depend on a libheif toolchain.

## Decision

Commit a handful of tiny valid pre-made HEIC samples (`tools/fixture-samples/`, produced once via macOS `sips`, a few hundred bytes each); the generator duplicates them across the tree.

## Consequences

Deterministic, no CI toolchain dependency; samples are data files under GPL-3.0 with the repo; produced by us, so no third-party licensing issue.
