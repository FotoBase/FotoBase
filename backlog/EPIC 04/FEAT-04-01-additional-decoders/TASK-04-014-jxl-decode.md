# task: implement jpeg xl decoding

**Feature:** [FEAT-04-01-additional-decoders](FEAT-04-01-additional-decoders.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** in review (2026-11-02)

## Description
Add JPEG XL support via `jxl-oxide` (pure Rust decoder, PLAN §4) so JXL files work for thumbnails and full export through the standard render/job pipelines (PLAN §6 P2b "JPEG XL").

## Functional specs
- `.jxl` files appear in the grid with thumbnails and open in the viewer like JPEG.
- JXL exports at full resolution with edits baked in, identical pipeline to other formats.

## Technical specs
- `jxl-oxide` crate in `crates/render`; register in the same decoder dispatch as libvips/LibRaw — no per-format branches in callers.
- Malformed JXL input must fail soft (§9 Robustness).

## Testing criteria (TDD)
- Integration: a JXL fixture produces a thumbnail and a full-resolution decode.
- Unit: truncated JXL bytes → Err, no panic.
- Integration: JXL with an applied recipe exports at full resolution.

## Acceptance criteria
- [x] 04-014: JXL decodes for thumbnails and full export through the shared pipelines, no special cases.
