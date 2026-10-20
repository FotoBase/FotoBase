# task: implement save-as and save-a-copy

**Feature:** [FEAT-04-06-export-save](FEAT-04-06-export-save.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** todo

## Description
"Save as…" (edited picture as a new file at a chosen location) and "Save a copy" (duplicate the current picture), both never touching the original (PLAN §2F, §6 P2b; §2 File menu).

## Functional specs
- Save as…: pick location/filename → write edited pixels there; original untouched.
- Save a copy: create a duplicate of the current (edited) picture next to the original or at the chosen location.
- Both write a sidecar with the new file; new files import into the catalog automatically.

## Technical specs
- Reuses the full-res render pipeline from 04-016; distinct only in destination handling.
- Original checksum must be untouched (test-enforced).
- New file gets its own sidecar; catalog scanner picks it up via the watcher.

## Testing criteria (TDD)
- Integration: Save as… and Save a copy → new files exist with correct pixels; original checksum unchanged before/after (§9 Data safety).
- Unit: destination collision → user-prompted or auto-suffixed name, never overwrite.

## Acceptance criteria
- [ ] 04-017: "Save as…" and "Save a copy" produce new files and never touch the original (checksum-verified).
