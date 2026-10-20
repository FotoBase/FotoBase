# task: move/rename re-association by (size, mtime, hash)

**Feature:** [FEAT-03-08-xmp-sidecar-layer](FEAT-03-08-xmp-sidecar-layer.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** todo

## Description
When a file is renamed or moved outside Fotobase, re-associate it with its sidecar and user data by (size, mtime, content hash) matching instead of treating it as new (PLAN §5 authority section; P2a acceptance). Implementation follows the 00-010 design doc (hard gate for EPIC-03).

## Functional specs
- Rename/move a photo in the OS file manager → after the next scan, stars/tags/captions/albums follow the file.
- The sidecar file follows the original (renamed alongside) per the 00-010 design.
- Ambiguous matches (two candidates) are surfaced, not silently resolved; no matches → treated as new.

## Technical specs

## Technical specs
- Match tiers: (known path) → (size + mtime window) → (lazy content hash); hashing stays lazy (PLAN §5, EPIC-02 02-009) — a moved file's hash may need computing on demand.
- `scanner` + `catalog`: on scan, an unmatched file triggers match lookup against missing-file records by size, then mtime window, then lazy hash.
- Sidecar rename/move performed by us post-association per design doc; hash cache updated.
- Picasa.ini-style legacy data unaffected.

## Testing criteria (TDD)
- Unit: matcher returns the right missing record for rename, move, and both-changed-mtime cases; returns None for a genuinely new file.
- Integration: rename fixture file in OS → rescan → stars/tags intact, sidecar follows, no duplicate row.

## Acceptance criteria
- [ ] 03-003: renamed/moved-outside files re-associate by (size, mtime, hash), not treated as new.
- [ ] 03-003: sidecar follows the original per 00-010.
- [ ] 03-003: ambiguous matches surfaced; no false merges.
