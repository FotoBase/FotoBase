# task: build recipe engine core

**Feature:** [FEAT-04-05-edit-recipe-engine](FEAT-04-05-edit-recipe-engine.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** in review (2026-10-30)

## Description
The versioned edit-recipe engine: an ordered ops model per the 00-011 spec, implemented as pure functions over linear-light buffers, with one code path for preview (screen-res) and export (full-res) (PLAN §5 "Edit recipe", §3 rule 5). This is the foundation every other EPIC-04 feature builds on.

## Functional specs
- An edit produces a recipe (ordered op list), never a modified original (§3 rule 1).
- The same recipe renders identically at preview and export resolution.
- Recipe `version` field enables forward migration when op algorithms change.

## Technical specs
- Op model in `crates/core`: `version`, `ops[]` of `{ type, params }` (e.g. `crop`, `straighten`, `auto_color`, `fill_light`, `temperature`, `sepia`).
- Ops are pure functions over linear-light image buffers; validation against the 00-011 JSON Schema.
- Preview renders at screen resolution; export at full resolution — same op code.

## Testing criteria (TDD)
- Unit: a recipe validates against the 00-011 JSON Schema; invalid ops rejected.
- Integration: same recipe applied at two resolutions produces matching output within per-pixel tolerance.
- Unit: recipe serialization round-trips (serialize → parse → identical op list).

## Acceptance criteria
- [x] 04-001: Versioned ops model implemented per 00-011 spec; ops are pure functions over linear-light buffers.
- [x] 04-001: Same op code path produces identical results at preview and export resolution.
