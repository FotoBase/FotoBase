# task: implement person albums and face filters

**Feature:** [FEAT-05-02-clustering-naming](FEAT-05-02-clustering-naming.md) · **Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** todo

## Description
Auto person albums, a "has faces" filter button, and person names searchable in FTS (PLAN §2G "Person albums"; §2C filter buttons; §6 P3 People).

## Functional specs
- Naming a person auto-creates an album of their confirmed photos (virtual, no copies per §2B Albums).
- "Has faces" filter button shows only photos with detected/named faces.
- Typing a person's name in the live search bar finds their photos.

## Technical specs
- Auto album = catalog-level album (`albums`/`album_files`) maintained on face confirm/rename; not written to sidecar `fb:Albums` (auto albums are derived).
- FTS: person names indexed in `files_fts` (people column, §5).
- Filter: SQL over `faces` join — "faces" filter per §2C.

## Testing criteria (TDD)
- Integration: confirm faces for "Anna" → album exists with those photos; FTS query "Anna" returns them.
- Integration: "has faces" filter on fixture → only photos with face rows.
- Integration: rename person → album and FTS results follow.

## Acceptance criteria
- [ ] 05-007: Person albums auto-created; "has faces" filter works; names searchable in FTS.
