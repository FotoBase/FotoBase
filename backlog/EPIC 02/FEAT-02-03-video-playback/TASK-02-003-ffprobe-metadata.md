# task: ffprobe metadata extraction and ffmpeg sidecar packaging

**Feature:** [FEAT-02-03-video-playback](FEAT-02-03-video-playback.md) · **Epic:** [EPIC-02 — Media & platform hardening](../EPIC-02.md) · **Status:** todo

## Description
Create the `video` crate (PLAN §3 module layout): extract duration, codec, and rotation via ffprobe at scan time into the catalog `metadata` table (PLAN §2 Video strategy), and package an LGPL ffmpeg sidecar binary with no non-free codecs.

## Functional specs
- Video files scanned into the library show duration and codec in the UI; playback honors rotation.
- Videos with missing/corrupt ffprobe data never crash the scanner (PLAN §9 robustness: malformed files must never crash).
- Shipped ffmpeg is LGPL, no non-free codecs; license manifest documented.

## Technical specs
- `video` crate wraps ffprobe (JSON output mode) and ffmpeg; called from the scanner for `media_type = video` files.
- Store duration, codec (name + profile), rotation in catalog `metadata`; `files.media_type` distinguishes video.
- ffmpeg sidecar: pinned LGPL build per OS, bundled via Tauri sidecar mechanism; version pinned and checked by CI (license manifest per §10 risk row).
- Probe failures are logged and the file marked, not fatal.

## Testing criteria (TDD)
- Unit: ffprobe JSON fixtures (h264, hevc, rotated clip, corrupt file) parse to expected metadata values; corrupt fixture yields a marked file, not a panic.
- Integration: scanning a fixture folder with mixed videos populates duration/codec/rotation rows.
- Integration: bundled ffmpeg reports expected version and no non-free encoders (`-encoders` output checked in CI).

## Acceptance criteria
- [ ] 02-003: duration, codec, rotation extracted at scan time into catalog `metadata`.
- [ ] 02-003: corrupt video files don't crash the scanner.
- [ ] 02-003: LGPL ffmpeg sidecar packaged for all three OSes, license documented.
