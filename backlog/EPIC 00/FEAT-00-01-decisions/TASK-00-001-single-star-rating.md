# task: decide single star vs 1–5 ratings, map to xmp:Rating

**Feature:** [FEAT-00-01-decisions](FEAT-00-01-decisions.md) · **Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** done

## Description
Resolve PLAN §11 Q1: how Fotobase represents favorites. Decided 2026-10-20: single star by default (Picasa-style), 1–5 ratings as an opt-in setting; both write the same `xmp:Rating` field so other tools (darktable, digiKam, Lightroom) see identical data either way.

## Functional specs
- Default UI shows a single star toggle per photo; starred maps to `xmp:Rating` = 5.
- Opt-in setting enables 1–5 rating input; values write directly to `xmp:Rating`.
- Switching between modes never rewrites existing sidecars; both modes read all 1–5 values.

## Technical specs
- XMP field per PLAN §5: `xmp:Rating` (star = 5, or 1–5 when ratings enabled).
- Setting persisted in app options; the catalog stores the raw rating value, UI derives star/rating presentation.

## Testing criteria (TDD)
- Unit: given a sidecar with `xmp:Rating=5`, the starred filter returns the photo in both UI modes.
- Unit: rating value round-trips through the metadata layer without mutation.

## Acceptance criteria
- [x] 00-001: Decision recorded in PLAN §11 Q1 (single star default, 1–5 opt-in, both map to `xmp:Rating`).
- [x] Date and rationale captured for the 2026-10-20 resolution.
