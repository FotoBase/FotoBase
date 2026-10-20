# task: add restore from backup sets

**Feature:** [FEAT-08-01-backup](FEAT-08-01-backup.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
Restore from backup sets with integrity validation before writing anything, and the byte-identical restore acceptance test (PLAN §6 P4 acceptance: "Backup → wipe → restore reproduces library and sidecars byte-identical"; §9).

## Functional specs
- Restore dialog lists available sets at a chosen location; user picks one.
- Set integrity (manifest hashes) is validated fully BEFORE any file is written; a corrupt set is rejected with specifics.
- Restoring over an existing library is explicit-confirm only; restore targets are the catalog location.
- After restore, the app reopens with the restored library intact (stars, tags, albums, edits).

## Technical specs
- Reverse of 08-014: walk manifest chain (full + incrementals), validate every hash, then write; staging area + atomic swap so a mid-restore failure never half-replaces the library.
- Byte-identical acceptance test (PLAN §9 Data safety): fixture library → backup → delete library + sidecars → restore → diff every file against pre-backup snapshot; also checksum originals before/after (they're untouched by definition — restore replaces catalog/sidecars/settings only).

## Testing criteria (TDD)
- Integration: the §9 byte-identical scenario — backup, wipe, restore, diff → zero mismatches.
- Integration: corrupt one file in the set → restore refuses before writing anything.
- Integration: mid-restore kill → previous library still intact (atomic staging).

## Acceptance criteria
- [ ] 08-015: Restore validates set integrity before writing anything and reproduces library and sidecars byte-identical (§9 acceptance test).
- [ ] 08-015: A mid-restore failure never leaves a half-restored library.
