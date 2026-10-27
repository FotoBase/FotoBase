# task: video player UI in grid and viewer

**Feature:** [FEAT-02-03-video-playback](FEAT-02-03-video-playback.md) · **Epic:** [EPIC-02 — Media & platform hardening](../EPIC-02.md) · **Status:** in review (2026-10-27)

## Description
Wire video playback into the UI: play from the grid (double-click) and in the single-photo viewer, choosing HTML5 (02-005) or `stream://` (02-006) transparently, honoring rotation metadata from 02-003.

## Functional specs
- Double-clicking a video in the grid opens the player; the viewer shows play/pause, scrub bar, duration from catalog metadata.
- Rotation from ffprobe metadata is honored (portrait phone clips display upright).
- Player works on all three OSes for common phone clips (H.264/HEVC).
- Closing the viewer stops playback and releases resources (transcode cancelled).

## Technical specs
- Svelte player component wrapping `<video>`; `src` resolved via `thumb://`-style helpers: direct file path for supported codecs, `stream://` otherwise.
- Rotation applied via CSS transform from catalog `metadata` (avoid re-encode).
- Keyboard shortcuts consistent with viewer (space = play/pause, arrows = seek).
- Cleanup on destroy: pause, clear src, let scheme handler cancel (02-006).

## Testing criteria (TDD)
- Unit: given codec support matrix + clip codec, the src resolver picks HTML5 vs `stream://` correctly.
- E2e: portrait H.264 fixture plays upright with correct aspect; play/pause/scrub work; closing the viewer stops playback (no audio continues).

## Acceptance criteria
- [x] 02-007: videos play from grid and viewer with working controls.
- [x] 02-007: rotation honored.
- [x] 02-007: playback stops cleanly when the viewer closes.
- [x] 02-007: H.264/HEVC phone clips play on all three OSes.
