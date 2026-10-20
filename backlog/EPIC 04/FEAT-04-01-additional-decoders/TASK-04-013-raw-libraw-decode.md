# task: implement raw decoding with libraw

**Feature:** [FEAT-04-01-additional-decoders](FEAT-04-01-additional-decoders.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** todo

## Description
Decode camera RAW files via LibRaw bindings in the `render` crate, with an embedded-JPEG-preview fallback for bodies LibRaw can't decode (PLAN §4, §10 risk row "LibRaw coverage"). RAWs feed the same thumbnail/preview/export pipelines as JPEG — no special cases.

## Functional specs
- RAW files (CR3, NEF, ARW, RAF, DNG, …) produce thumbnails and full-resolution previews like any other image.
- Unsupported/corrupt RAW: no crash; embedded JPEG preview is used, or the file shows a degraded state — never a hard failure.
- RAW+JPEG pairs stack and export correctly (epic acceptance) — depends on stacking from EPIC-03.

## Technical specs
- LibRaw via own bindings in `crates/render` (PLAN §3 module layout); decode off the UI thread via the job queue (§3 rule 4).
- Fallback path: extract embedded JPEG from the RAW container when LibRaw errors.
- Fuzzing requirement: malformed camera media must never crash the scanner (§9 Robustness).
- License: LibRaw is LGPL/CPL dual — keep it an isolated crate boundary.

## Testing criteria (TDD)
- Integration: fixtures from each major vendor (Canon/Nikon/Sony/Fujifilm) decode to a thumbnail within 2s.
- Integration: a fixture with a deliberately corrupted RAW header falls back to embedded JPEG without error.
- Unit: fuzz corpus of malformed RAW bytes → decoder returns Err, never panics.
- E2e: RAW+JPEG pair from fixture set stacks and exports (with FEAT-04-06 export).

## Acceptance criteria
- [ ] 04-013: RAW files from each major vendor decode via LibRaw in thumbnail, preview, and export paths.
- [ ] 04-013: Unsupported bodies fall back to embedded JPEG preview; no hard failures.
- [ ] 04-013: RAW+JPEG pair stacks and exports correctly per vendor fixture.
