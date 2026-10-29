# task: picasa.ini ingest on first scan

**Feature:** [FEAT-03-04-picasa-migration-portability](FEAT-03-04-picasa-migration-portability.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in review (2026-10-29)

## Description
On first scan of a folder, read `Picasa.ini` files and convert legacy stars, albums, and faces into XMP sidecars — the "your old library, restored" onboarding hook (PLAN §1 onboarding, §6 P2a). Depends on the sidecar merge-on-write design (00-010, hard gate for EPIC-03).

## Functional specs
- On first scan of a folder containing `Picasa.ini`, Fotobase offers migration; converted data lands in standard XMP (rating, tags, face regions).
- Legacy stars → `xmp:Rating`; albums → `fb:Albums`; faces → MWG `mwg-rs:Regions` (PLAN §5 mapping).
- Re-running migration is idempotent (no duplicates); user can decline per folder.

## Technical specs
- `metadata`/`scanner`: Picasa.ini parser (INI format with Picasa's key=value quirks, e.g. `P2WS` album encoding); run as scan-time job.
- Writes go through the 03-001 merge-on-write path (preserve unknown XMP fields; ini files themselves never modified/deleted).
- Face-region coordinate conversion: Picasa's percent-based rects → MWG normalized rects.

## Testing criteria (TDD)
- Unit: parser fixture covering stars, album membership, face names; percent→MWG rect conversion math.
- Integration: real-world Picasa library fixture migrates with zero loss vs a hand-verified expected sidecar set; second scan changes nothing.

## Acceptance criteria
- [x] 03-022: first-scan ingest converts legacy stars, albums, faces to XMP.
- [x] 03-022: a real Picasa library test case migrates with no loss.
- [x] 03-022: idempotent; `.ini` files untouched.
