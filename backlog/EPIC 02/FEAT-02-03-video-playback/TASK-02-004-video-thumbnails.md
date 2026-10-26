# task: video thumbnails via ffmpeg frame extraction

**Feature:** [FEAT-02-03-video-playback](FEAT-02-03-video-playback.md) · **Epic:** [EPIC-02 — Media & platform hardening](../EPIC-02.md) · **Status:** in review (2026-10-26)

## Description
Extract video thumbnail frames with ffmpeg into the same thumbnail store as images, served by the same `thumb://` scheme, at lower queue priority than visible images (PLAN §2 Video strategy, thumbnails bullet).

## Functional specs
- Videos show a thumbnail in the grid, same appearance/zoom behavior as image thumbnails.
- Video thumbnail jobs never block visible image thumbnails (lower priority in the queue).
- Failed extraction (corrupt/unsupported file) falls back to a placeholder icon.

## Technical specs
- `video` crate: extract a frame at ~10% of duration via ffmpeg; feed into the existing `render`/thumbnail cache path (same cache keys, same `thumb://` handler — no new serve path).
- `jobs` crate: priority class below visible-image thumbnails (PLAN §3 rule 4: visible items first, background last); throttle on battery applies.

## Testing criteria (TDD)
- Unit: a video path enqueues a thumbnail job with lower priority than a visible image job (assert queue ordering).
- Integration: fixture clip produces a cached thumbnail via `thumb://`; corrupt clip yields placeholder, no crash.

## Acceptance criteria
- [x] 02-004: videos get thumbnails in the shared cache via `thumb://`.
- [x] 02-004: video thumbnail jobs run at lower priority than visible image jobs.
- [x] 02-004: corrupt videos fall back to a placeholder without errors surfacing to the user.
