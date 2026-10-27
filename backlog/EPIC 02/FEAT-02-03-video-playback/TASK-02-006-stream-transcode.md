# task: stream:// on-the-fly transcode fallback

**Feature:** [FEAT-02-03-video-playback](FEAT-02-03-video-playback.md) · **Epic:** [EPIC-02 — Media & platform hardening](../EPIC-02.md) · **Status:** in review (2026-10-27)

## Description
Implement the `stream://` custom Tauri URI scheme: when the codec probe (02-005) says the WebView can't decode a clip, transcode on the fly with the ffmpeg sidecar to a supported intermediate (H.264/mp4 or VP9/webm) and stream it to the player — no full-file temp copy (PLAN §2 Video strategy).

## Functional specs
- Unsupported-codec clips start playback within ~2 s.
- Playback streams; disk does not accumulate a full transcode copy.
- Stopping playback cancels the transcode process cleanly (no orphan ffmpeg).
- Seeking works (restart transcode at keyframe near the seek point).

## Technical specs
- Tauri custom protocol handler `stream://<file-id>` in `app/src-tauri`; spawns `ffmpeg -i <src> ... -f mp4/webm -movflags frag_keyframe+empty_moov -` piped to the response.
- Target codec chosen from the probe matrix (H.264/mp4 preferred, VP9/webm fallback); audio transcoded to AAC/Opus accordingly.
- Process registry keyed by file-id; player stop/destroy kills the process; idle timeout as backstop.
- Follows PLAN §3 rule 3 (no media over IPC — bytes go through the scheme handler).

## Testing criteria (TDD)
- Integration: requesting `stream://` for an HEVC fixture returns a playable mp4/webm stream; first bytes arrive < 2 s.
- Integration: aborting the request kills the ffmpeg child (assert no process remains).
- E2e: on a WebView without HEVC, the HEVC fixture plays end-to-end.

## Acceptance criteria
- [x] 02-006: unsupported-codec clips play via `stream://` transcode fallback.
- [x] 02-006: playback starts within ~2 s and streams (no full temp file).
- [x] 02-006: stopping playback cancels the transcode cleanly.
- [x] 02-006: seeking works on transcoded playback.
