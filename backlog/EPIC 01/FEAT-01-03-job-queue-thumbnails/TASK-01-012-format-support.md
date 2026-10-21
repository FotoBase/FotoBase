# task: support JPEG/PNG/GIF/BMP/TIFF/WebP/HEIC/AVIF decoders

**Feature:** [FEAT-01-03-job-queue-thumbnails](FEAT-01-03-job-queue-thumbnails.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in review (2026-10-21)

## Description
Wire format support per PLAN §2A/§4: JPEG, PNG, GIF, BMP, TIFF, WebP decoded via zune-jpeg/`image`; HEIC and AVIF via `libheif-rs`; PSD thumbnails only if cheap. All produce correct thumbnails through the 01-010 pipeline.

## Functional specs
- Each listed format shows a correct thumbnail in the grid and opens in the viewer.
- Malformed files of any format are skipped gracefully — never crash the scanner or render worker (§9 fuzz goal).
- PSD: embedded preview thumbnail if it comes cheap; otherwise deferred (RAW/JXL arrive in P2).

## Technical specs
- Decoder dispatch table in `render`; `libheif-rs` for HEIC/AVIF (§4).
- Extend the 00-013 fixture with one file per supported format for tests.

## Testing criteria (TDD)
- Unit: per format, a fixture file decodes and produces a thumbnail of expected dimensions.
- Unit: truncated/corrupt file per format returns a decode error, not a panic.

## Acceptance criteria
- [x] 01-012: All listed formats (incl. HEIC/AVIF via libheif-rs) produce correct thumbnails; corrupt files never crash.
