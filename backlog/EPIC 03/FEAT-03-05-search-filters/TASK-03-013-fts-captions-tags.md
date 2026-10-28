# task: extend FTS to captions and tags

**Feature:** [FEAT-03-05-search-filters](FEAT-03-05-search-filters.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in progress (2026-10-28)

## Description
Extend live search beyond filenames/folders (P1 scope) to captions and tags via the existing FTS5 index (PLAN §5 catalog: `files_fts` over filename, folder, caption, tags, people, place; §2 C live search). Keeps as-you-type performance on the 50k fixture.

## Functional specs
- Typing matches captions and tag names in addition to filenames/folders; results group sensibly as today.
- Search stays as-you-type with no jank on the 50k-photo fixture.
- New/edited captions and tags become searchable without a manual rebuild.

## Technical specs
- FTS5 table already has caption/tags columns; wire index updates into the sidecar write-through path (03-002) so FTS rows refresh on every user-data mutation.
- Query path unchanged (P1 live search); just more columns matched.
- Rebuild repopulates FTS from sidecars (part of standard rebuild).

## Testing criteria (TDD)
- Unit: FTS query for a caption substring and a tag name returns the expected fixture photos.
- Integration: edit a caption → new search term matches immediately without rebuild.
- Perf: 50k fixture keystroke latency within the P1 search budget (assert under threshold).

## Acceptance criteria
- [x] 03-013: live search matches captions and tags as-you-type.
- [x] 03-013: edits are searchable without manual rebuild.
- [x] 03-013: no jank on the 50k fixture.
