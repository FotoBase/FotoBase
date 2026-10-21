# task: implement thumb:// uri scheme handler

**Feature:** [FEAT-01-03-job-queue-thumbnails](FEAT-01-03-job-queue-thumbnails.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in progress (2026-10-21)

## Description
Implement the custom Tauri URI scheme (PLAN §3 rule 3): `thumb://` serves cached thumbnails to the WebView directly from disk — zero image bytes cross IPC.

## Functional specs
- The grid sets `<img src="thumb://<hash>">` and the WebView loads bytes from the cache.
- A cache-miss returns a placeholder (and triggers a render job), never an error dialog.
- No Tauri command ever returns image bytes.

## Technical specs
- Tauri v2 URI scheme registration in `app/src-tauri/`; handler reads from the 01-010 cache, resolving hash → cache path.
- Same scheme later extended with `preview://` for full previews (§3 diagram) — P1 only needs `thumb://`.

## Testing criteria (TDD)
- Unit: request for a cached hash returns the cached bytes; request for unknown hash returns placeholder status.
- E2E: grid shows thumbnails; a debug log/counter asserts no image payload crossed an invoke boundary.

## Acceptance criteria
- [x] 01-011: `thumb://` scheme serves thumbnails from cache; no image bytes over IPC.
