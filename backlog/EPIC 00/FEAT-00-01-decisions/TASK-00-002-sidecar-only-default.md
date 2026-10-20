# task: decide sidecar-only metadata with opt-in embed on save

**Feature:** [FEAT-00-01-decisions](FEAT-00-01-decisions.md) · **Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** done

## Description
Resolve PLAN §11 Q2: where "Save" writes metadata. Decided 2026-10-20: XMP sidecars only by default (`.jpg.xmp` next to the original, per §5); an opt-in setting embeds XMP into the original file on Save. Keeps the non-destructive default (§3 rule 1) while allowing single-file workflows.

## Functional specs
- By default, all user metadata (rating, tags, caption, GPS, faces, recipe) goes to the sidecar; originals untouched.
- Opt-in setting: on explicit "Save", XMP is embedded into the original file in addition to/instead of the sidecar.
- Interop: sidecars readable by darktable/digiKam/Lightroom per §1 interoperable goal.

## Technical specs
- Sidecar naming `IMG_1234.jpg.xmp` per §5; write path via exiv2 bindings (§4).
- Embed path uses exiv2 in-file XMP write; only on explicit Save.

## Testing criteria (TDD)
- Unit: after a metadata edit with default settings, the original file's checksum is unchanged (§9 data-safety check).
- Integration: with embed enabled, a Save produces a file whose embedded XMP contains the new rating.

## Acceptance criteria
- [x] 00-002: Decision recorded in PLAN §11 Q2 (sidecars only by default, opt-in embed on Save).
- [x] Date and rationale captured for the 2026-10-20 resolution.
