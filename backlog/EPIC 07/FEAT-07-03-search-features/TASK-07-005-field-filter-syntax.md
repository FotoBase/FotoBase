# task: add advanced search field filter syntax

**Feature:** [FEAT-07-03-search-features](FEAT-07-03-search-features.md) · **Epic:** [EPIC-07 — Smart search](../EPIC-07.md) · **Status:** todo

## Description
Extend the search bar with a field-filter grammar — `person:`, `camera:`, `before:` (and symmetric `after:`) — combinable with free text and semantic results in one query (PLAN §2 2026 additions: "Advanced search syntax: `person:anna camera:X100V before:2023`").

## Functional specs
- `person:anna camera:X100V before:2023 sunset` filters to photos of that person, that camera, before that date, matching "sunset" (FTS + semantic).
- Unknown/malformed filters fall back to literal text instead of erroring.
- Filters compose with all existing filters (starred, date slider, map selection — PLAN §6 P3 map selection/filter integration is the sibling).
- Person filter works from named faces (EPIC-05 People track, MWG regions); camera from catalog EXIF metadata.

## Technical specs
- Small tokenizer/parser in the `catalog` crate: `field:value` tokens → SQL predicates (`people`, `metadata` tables) composed with the FTS + semantic blend from 07-003; free text → existing blend.
- Fields: `person:` (name lookup → face join), `camera:` (EXIF Make/Model substring), `before:`/`after:` (ISO date or YYYY against `taken_at`).
- Pure, unit-tested parse → structured AST → SQL builder; no regex soup.

## Testing criteria (TDD)
- Unit: parser handles `person:anna camera:X100V before:2023 sunset`, quoted values, and garbage input (treated as text).
- Integration: fixture with two people and two cameras — `person:` + `camera:` + `before:` combined returns exactly the intersecting set.
- E2E: typing a field filter in the search bar narrows the grid live.

## Acceptance criteria
- [ ] 07-005: Field filters (`person:`, `camera:`, `before:`) combine with free text and semantic results in one query syntax.
- [ ] 07-005: Malformed filter tokens degrade to literal text search without errors.
