# task: stacking — RAW+JPEG pairs, bursts, edited versions

**Feature:** [FEAT-03-01-metadata-editing-stacking](FEAT-03-01-metadata-editing-stacking.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** todo

## Description
Group RAW+JPEG pairs, burst sets, and edited versions under one thumbnail with a stack picker (PLAN §6 P2a). Stacking is metadata-only — no file moves, no original modification. Depends on the sidecar design doc (00-010, hard gate for EPIC-03) since stack membership is user data that must survive rebuild.

## Functional specs
- Auto-detect RAW+JPEG pairs (same stem, same timestamp window) and offer to stack; bursts stackable manually.
- Edited versions (files carrying `fb:Recipe`) stack under their original.
- One thumbnail per stack; a picker badge expands the stack; top-of-stack is switchable.
- Unstack is lossless; stacking changes nothing on disk.

## Technical specs
- Stack membership stored in the sidecar per the 00-010 design and PLAN §11 Q5: `fb:StackId` + `fb:StackRole` fields on each member (decided 2026-10-20); catalog holds a derived stack index.
- Pair detection heuristic: filename stem match + taken_at within N seconds; calibration knob for N (hardware clocks drift).
- Grid/viewer render stack badge; picker is a small overlay component; stack state is a normal query dimension.

## Testing criteria (TDD)
- Unit: pair detector matches fixture RAW+JPEG stems and rejects different-timestamp false positives.
- Integration: stack two photos → delete catalog → rebuild → stack survives.
- E2e: grid shows one thumbnail with badge; picker expands; unstack restores both thumbnails.

## Acceptance criteria
- [ ] 03-021: RAW+JPEG pairs, bursts, and edited versions stack under one thumbnail.
- [ ] 03-021: stack picker works; top-of-stack switchable.
- [ ] 03-021: stacking is metadata-only; rebuildable; originals unchanged (checksum).
