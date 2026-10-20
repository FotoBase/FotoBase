# task: build near-duplicate finder

**Feature:** [FEAT-04-04-duplicates](FEAT-04-04-duplicates.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** todo

## Description
Perceptual-hash near-duplicate finder across the catalog with a review UI (keep / suggest-delete); nothing is deleted without explicit user action (PLAN §2 2026 additions "Near-duplicate detection"; §4 `image_hasher`; §6 P2b).

## Functional specs
- Tools-menu finder groups near-duplicates across the whole catalog into reviewable sets.
- Review UI per group: pick keepers; suggested deletes go to trash only on explicit confirmation.
- Hashing runs as a background job; 50k fixture without UI block.

## Technical specs
- `image_hasher` crate for perceptual hashing; hashes stored in the catalog `files` (derived data — cache, recomputable, §5 Authority).
- Runs in the job queue at background priority (§3 rule 4).
- Deletion routes through the trash-with-confirmation path (§2 File menu "Delete from disk").

## Testing criteria (TDD)
- Integration: known-duplicate fixture set (resized/re-compressed variants) → full recall, false-positive rate recorded.
- Integration: 50k fixture hashing completes as a background job; UI stays responsive (§9 Performance).
- E2e: run finder, review a group, confirm delete → file in trash; cancel → nothing deleted.

## Acceptance criteria
- [ ] 04-019: Near-duplicate groups found across the catalog via perceptual hash; review UI with keep/suggest-delete.
- [ ] 04-019: Nothing deleted without explicit user action; recall/FP numbers recorded on the fixture set.
