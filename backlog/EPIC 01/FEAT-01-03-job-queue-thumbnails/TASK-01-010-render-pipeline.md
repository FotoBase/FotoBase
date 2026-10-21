# task: build render crate — decode + thumbnail pipeline + cache

**Feature:** [FEAT-01-03-job-queue-thumbnails](FEAT-01-03-job-queue-thumbnails.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in review (2026-10-21)

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

## Deviations
- libvips is **not** used for resize. The workspace denies `unsafe_code`, which rules out libvips FFI bindings, and libvips would add a system dependency to all CI platforms (§4, §10). Resize uses pure-Rust `fast_image_resize` (Lanczos3) instead. The recipe engine (01-045/04-05) owns the linear-light ops, so nothing in §5 depends on libvips being the resizer. If later benchmarks (§6 P1) show the resize path is a bottleneck, revisit via a new ADR.

## Acceptance criteria
- [x] 01-010: `render` crate decodes via zune-jpeg/image/libvips, produces thumbnails via the job queue, caches on disk keyed by content hash.
