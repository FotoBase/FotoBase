# task: add reset faces action

**Feature:** [FEAT-05-03-storage-ui](FEAT-05-03-storage-ui.md) · **Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** todo

## Description
"Reset faces" per photo from the picture context menu: clear detected/named face regions for that photo (PLAN §2 Picture context menu "Reset faces"; §6 P3 People).

## Functional specs
- Context-menu item clears all face regions and names for the photo.
- Face rows and MWG regions removed; photo leaves "has faces" filter and any person albums.
- Destructive action → confirmation or trash/undo window per EPIC-03 catalog-trash conventions.

## Technical specs
- Deletes `faces` rows + MWG regions from the sidecar (merge-on-write); embeddings row removed.
- Routes through the catalog trash/soft-delete mechanism from P2a where applicable.

## Testing criteria (TDD)
- Integration: photo with named faces → Reset → sidecar has no regions, catalog has no face rows, photo absent from person album.
- E2e: context menu → Reset faces → confirmation → state cleared.

## Acceptance criteria
- [ ] 05-009: "Reset faces" clears detected/named face regions per photo via context menu.
