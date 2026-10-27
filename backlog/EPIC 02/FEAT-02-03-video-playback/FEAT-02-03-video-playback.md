# feat: video playback (§2 Video strategy)

**Epic:** [EPIC-02 — Media & platform hardening](../EPIC-02.md) · **Status:** planned

## Acceptance criteria

- H.264/HEVC phone clips play on all three OSes — natively where the OS WebView has the codec, via `stream://` transcode fallback where it doesn't.
- ffprobe metadata (duration, codec, rotation) is in the catalog at scan time; the player honors rotation; video thumbnails share the image thumbnail cache at lower queue priority.
- Transcode fallback starts within ~2 s, streams (no full-file temp copy), and is cancelled cleanly when playback stops.
- Shipped ffmpeg is LGPL with no non-free codecs; codec probe results are visible for debugging.

- [x] `02-003` (M) `video` crate: ffprobe metadata (duration, codec, rotation) at scan time into catalog `metadata`; ffmpeg sidecar packaging (LGPL build, no non-free codecs).
- [x] `02-004` (M) Video thumbnails: ffmpeg frame extraction into the same thumbnail cache, lower priority in job queue.
- [x] `02-005` (M) Startup codec-capability probe; HTML5 `<video>` first.
- [x] `02-006` (L) `stream://` custom URI scheme: on-the-fly transcode fallback (H.264/mp4 or VP9/webm) for unsupported codecs, streamed to player.
- [ ] `02-007` (M) Video player UI in grid/viewer; rotation honored.

## Tasks

- [ ] [TASK-02-003-ffprobe-metadata.md](TASK-02-003-ffprobe-metadata.md) — ffprobe metadata extraction and ffmpeg sidecar packaging
- [ ] [TASK-02-004-video-thumbnails.md](TASK-02-004-video-thumbnails.md) — video thumbnails via ffmpeg frame extraction
- [x] [TASK-02-005-codec-probe.md](TASK-02-005-codec-probe.md) — startup codec-capability probe, HTML5 first
- [ ] [TASK-02-006-stream-transcode.md](TASK-02-006-stream-transcode.md) — stream:// on-the-fly transcode fallback
- [ ] [TASK-02-007-video-player-ui.md](TASK-02-007-video-player-ui.md) — video player UI in grid and viewer
