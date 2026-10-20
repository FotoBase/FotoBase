# task: metadata crate — own exiv2 bindings and sidecar read/write

**Feature:** [FEAT-03-08-xmp-sidecar-layer](FEAT-03-08-xmp-sidecar-layer.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** todo

## Description
Build the `metadata` crate: own exiv2 bindings (rexiv2 fork per PLAN §10 mitigation), read/write XMP sidecars next to originals, with merge-on-write per the 00-010 design doc ([`docs/sidecar-merge-on-write.md`](../../../docs/sidecar-merge-on-write.md) — gate satisfied 2026-10-20). Merge-on-write preserves unknown fields and backs up the previous sidecar version before every write (§5).

## Functional specs
- Reading a sidecar yields typed access to rating, tags, caption, GPS, albums, recipes, plus pass-through of unknown fields.
- Writing is read-modify-write: fields from other apps (digiKam, Syncthing) survive our write.
- Previous sidecar version is backed up before every write.
- Missing sidecar is created on first write; malformed sidecar never crashes the reader (§9 fuzz row).

## Technical specs
- `metadata` crate with a crate-internal trait so the exiv2 backend can be swapped (§10); `kamadak-exif` as read-only fallback.
- rexiv2 fork under our control; track exiv2 releases.
- Backup scheme per 00-010 (`.originals`-style per §5); merge = parse full XMP tree → modify known nodes → serialize.
- Fuzz the parser (§9: camera media is untrusted input).

## Testing criteria (TDD)
- Unit: write rating to a sidecar containing an unknown digiKam field → re-read shows both; backup file exists and matches the pre-write content.
- Unit: malformed XMP fixture returns an error, not a panic (fuzz corpus).
- Integration: read → write → read round-trip preserves field order/unknowns byte-stably where possible.

## Acceptance criteria
- [ ] 03-001: own exiv2 bindings (rexiv2 fork) with swappable backend trait.
- [ ] 03-001: merge-on-write preserves unknown fields; previous version backed up.
- [ ] 03-001: malformed sidecars never crash the scanner.
- [x] 03-001: 00-010 design doc landed before implementation (hard gate).
