# task: build portable gift export (folder/zip/usb)

**Feature:** [FEAT-10-03-f4-gift-export](FEAT-10-03-f4-gift-export.md) · **Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (deferred)

## Description
Export an album to folder / zip / USB stick with an offline HTML viewer (reuses the F1.1 generator from FEAT-10-01 task 01), full-res photos, captions, and a slideshow with music (PLAN §7 F4; successor to Gift CD). Depends on EPIC-04 export.

## Functional specs
- Export produces a folder/zip/USB layout openable via `index.html` with no installation.
- Contains full-res images and captions.
- Slideshow plays with music from the viewer.

## Technical specs
- Same static-gallery generator as F1.1 (shared module, not a copy); export through the EPIC-04 export pipeline.
- Music playback via standard HTML audio in the tiny JS viewer.

## Testing criteria (TDD)
- integration: export a fixture album to a temp folder; assert full-res files, captions, index.html present.
- e2e: open index.html from disk; slideshow starts and plays audio.

## Acceptance criteria
- [ ] F4-001: Folder/zip/USB export contains full-res images, captions, and an offline HTML viewer with a music slideshow — no installation needed.
