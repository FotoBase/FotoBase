# EPIC-02 — Media & platform hardening

**Plan ref:** §6 Phase 1.5 · **Size:** M · **Depends on:** EPIC-01

Each item is independently shippable. Ordered: color management → video → NAS.

## Acceptance

- H.264/HEVC phone clips play on all three OSes.
- Preview colors match the OS viewer on macOS/Windows (display-profile aware).
- Watched SMB share: network unplugged mid-scan → scan pauses cleanly, resumes on reconnect, no UI hang.

## feat: color management (P1.5 display scope)

- [x] `02-001` (M) Read display ICC profile (macOS/Windows); color-managed preview path in render crate; sRGB fallback on Linux.
- [x] `02-002` (S) Golden-image tests for display transform; visual comparison vs OS viewers.

## feat: video playback (§2 Video strategy)

- [ ] `02-003` (M) `video` crate: ffprobe metadata (duration, codec, rotation) at scan time into catalog `metadata`; ffmpeg sidecar packaging (LGPL build, no non-free codecs).
- [ ] `02-004` (M) Video thumbnails: ffmpeg frame extraction into the same thumbnail cache, lower priority in job queue.
- [ ] `02-005` (M) Startup codec-capability probe; HTML5 `<video>` first.
- [ ] `02-006` (L) `stream://` custom URI scheme: on-the-fly transcode fallback (H.264/mp4 or VP9/webm) for unsupported codecs, streamed to player.
- [ ] `02-007` (M) Video player UI in grid/viewer; rotation honored.

## feat: network drives (NAS)

- [ ] `02-008` (M) Polling watcher for SMB/NFS paths (adaptive interval, mtime+size) — `notify` never fires over network FS.
- [ ] `02-009` (M) Throttled scanning for network folders: IO limits, pause-on-disconnect, clean resume, no UI hang; hash lazily.
- [ ] `02-010` (S) Disconnect/reconnect test harness with virtual SMB share in CI (or documented manual protocol).
