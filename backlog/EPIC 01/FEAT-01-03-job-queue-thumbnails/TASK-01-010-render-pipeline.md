# task: build render crate — decode + thumbnail pipeline + cache

**Feature:** [FEAT-01-03-job-queue-thumbnails](FEAT-01-03-job-queue-thumbnails.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** todo

## Description
Implement the `render` crate per PLAN §4: fast decode (zune-jpeg/image hot path, libvips via own bindings for resize), the thumbnail pipeline running inside the job queue, and an on-disk cache keyed by content hash so re-scans never re-render.

## Functional specs
- Thumbnails appear for visible photos before background ones; UI never blocks on decode (§3 rule 4).
- Cache hit on re-scan: a file whose content hash is already cached renders instantly.
- 50k library: first thumbnails visible < 2 s after scan start (§6 P1 acceptance).

## Technical specs
- zune-jpeg + `image` for JPEG/PNG; libvips (own thin bindings, §4) + `fast_image_resize` for resize; formats per 01-012.
- Cache keyed by content hash (§5 derived data — never written to sidecars); cache location under the catalog directory.

## Testing criteria (TDD)
- Unit: generate a thumbnail twice with the same content hash ⇒ second call is a cache hit (no decode).
- Integration: on the 1k fixture, thumbnails complete with zero failed decodes; malformed fixture file is skipped without crashing (§9 robustness).

## Acceptance criteria
- [ ] 01-010: `render` crate decodes via zune-jpeg/image/libvips, produces thumbnails via the job queue, caches on disk keyed by content hash.
