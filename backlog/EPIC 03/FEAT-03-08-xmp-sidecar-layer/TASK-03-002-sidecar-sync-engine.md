# task: sidecar sync engine — catalog ↔ sidecar authority

**Feature:** [FEAT-03-08-xmp-sidecar-layer](FEAT-03-08-xmp-sidecar-layer.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** todo

## Description
The sync engine implementing PLAN §5 authority rules: sidecars are authoritative for user data (rating, tags, caption, GPS, faces, recipe, stack membership, hidden flag — §11 Q5/Q6); the catalog is derived cache. Write-through on every user-data mutation; sidecar wins on conflict. Depends on 03-001 and the 00-010 design doc (hard gate for EPIC-03).

## Functional specs
- Every user-data mutation (star, tag, caption, album, GPS, stack) writes the sidecar in the same operation; catalog follows.
- External sidecar edits (user edits in digiKam) are converged on next read — sidecar overwrites catalog.
- Derived data (thumbnails, FTS, hashes) is never written back to sidecars.
- Rebuild from sidecars alone restores all user data.

## Technical specs
- `catalog`/`metadata`: single mutation pathway all UI commands route through; write-through = sidecar write (03-001) + catalog row update in one logical transaction; on sidecar-write failure the mutation fails loudly (no silent divergence).
- Conflict detection on read (sidecar mtime > last-sync) triggers re-read per §5 conflict rule.
- Incremental rebuild by mtime+size skip; full rebuild explicit (§5).

## Testing criteria (TDD)
- Unit: given catalog rating 3 and sidecar rating 5, read converges catalog to 5.
- Integration: star a photo → sidecar file changes in the same operation; mutate via digiKam fixture → next read converges.
- Integration: sidecar write failure (read-only dir) → mutation surfaces an error, catalog unchanged.

## Acceptance criteria
- [ ] 03-002: write-through on every user-data mutation.
- [ ] 03-002: sidecar wins on conflict; derived data never written to sidecars.
- [ ] 03-002: rebuild from sidecars restores user data (feeds 03-026 harness).
