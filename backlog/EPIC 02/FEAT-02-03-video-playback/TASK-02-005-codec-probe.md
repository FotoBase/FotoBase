# task: startup codec-capability probe, HTML5 first

**Feature:** [FEAT-02-03-video-playback](FEAT-02-03-video-playback.md) · **Epic:** [EPIC-02 — Media & platform hardening](../EPIC-02.md) · **Status:** todo

## Description
At startup, probe the OS WebView for codec support (PLAN §10: "WebKitGTK ships without H.264/HEVC on several distros") so playback decisions are data-driven: HTML5 `<video>` is the primary path when the codec is supported.

## Functional specs
- Probe runs once at startup (fast, no visible delay); results cached per session.
- A clip whose codec is supported plays directly via HTML5 `<video>`.
- Probe results are visible for debugging (e.g. in an about/diagnostics view or log).
- Probe never blocks app startup.

## Technical specs
- Frontend: `MediaSource.isTypeSupported()` / `canPlayType()` for the codec set we care about (H.264, HEVC, VP9, AV1); report to Rust via a startup command.
- Store per-codec boolean in app state; player component consults it before choosing HTML5 vs `stream://` (02-006).
- Log the matrix at startup (debug level) and surface in diagnostics.

## Testing criteria (TDD)
- Unit: probe result parsing maps sample `canPlayType` outputs to support booleans correctly.
- E2e: on a WebView with H.264 support, an H.264 fixture plays via plain `<video>` (no `stream://` request observed); on one without, `stream://` is requested (once 02-006 lands; test the decision function until then).

## Acceptance criteria
- [ ] 02-005: codec support probed at startup, results cached.
- [ ] 02-005: supported codecs play via HTML5 `<video>` with no transcode.
- [ ] 02-005: probe matrix is visible for debugging.
