# ADR-0007: Hidden flag storage

**Date:** 2026-10-20 · **Status:** accepted (PLAN §11 Q6)

## Context

Hidden folders/photos must disappear from the library only, survive rebuild, and not touch files on disk.

## Decision

Photo hide → `fb:Hidden` in the photo's sidecar. Folder hide → a `hidden` flag in one folder-level `.fotobase` JSON metadata file; member photos inherit at scan time.

## Consequences

Rebuild-safe without writing N sidecars per folder-hide; format defined in docs/sidecar-merge-on-write.md §2.
