# task: albums with fb:Albums sidecar namespace

**Feature:** [FEAT-03-06-user-organization-data](FEAT-03-06-user-organization-data.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in review (2026-10-28)

## Description
Virtual albums: create (name/date/place/description), delete, add/remove photos; membership stored in the custom `fb:Albums` XMP namespace so albums rebuild from sidecars (PLAN §2 B Albums, §5 mapping). Depends on the sidecar merge-on-write design (00-010, hard gate for EPIC-03).

## Functional specs
- Create album with name, date, optional place and description; delete removes the album only.
- A photo can be in many albums without copying; add/remove from grid, viewer, or tray.
- Album list UI (sidebar) with counts; albums are pure virtual — originals never touched.
- Membership survives delete-catalog → rebuild.

## Technical specs
- `fb:Albums` namespace per §5 (album IDs on each member's sidecar) + catalog `albums`/`album_files` tables as derived cache.
- Album metadata (name/date/place/description) itself must be rebuildable — store album definitions in a catalog-location sidecar/JSON per the 00-010 design.
- Album cover photo = first added or user-picked.

## Testing criteria (TDD)
- Unit: add/remove photo writes/removes the expected `fb:Albums` fragment.
- Integration: two photos in one album → delete catalog → rebuild → album and membership restored.
- E2e: create album, add from two folders, album list shows correct count; delete album leaves photos intact.

## Acceptance criteria
- [x] 03-008: albums create/delete/add/remove work; name/date/place/description supported.
- [x] 03-008: membership in `fb:Albums`, rebuildable after catalog delete.
- [x] 03-008: album list UI present; originals never touched.
