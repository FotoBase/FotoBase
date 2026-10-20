# task: publish edit-recipe JSON spec v1 + schema

**Feature:** [FEAT-00-02-design-docs](FEAT-00-02-design-docs.md) · **Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** done (2026-10-20) → [`docs/edit-recipe-spec-v1.md`](../../../docs/edit-recipe-spec-v1.md), [`recipe-v1.schema.json`](../../../docs/recipe-v1.schema.json), [`recipe-v1.examples.json`](../../../docs/recipe-v1.examples.json)

## Description
Publish the versioned edit-recipe spec (PLAN §5 anti-lock-in guarantee): the JSON format stored in the `fb:Recipe` XMP namespace, plus a JSON Schema in `docs/`, so third parties can read/apply Fotobase recipes. Needed before EPIC-04 freezes op types.

## Functional specs
- Spec defines `version` and ordered `ops[]` where each op is `{ type, params }` (e.g. `crop`, `straighten`, `auto_color`, `fill_light`, `temperature`, `sepia`).
- Recipe versioning enables migrations when an op's algorithm changes.
- A third party could write a compatible recipe reader from the spec alone.

## Technical specs
- Stored as versioned JSON in custom namespace `fb:Recipe` (§5).
- JSON Schema published in `docs/`; ops are pure functions over a linear-light buffer, same code path for preview and export.

## Testing criteria (TDD)
- Unit: schema validates example recipes for every v1 op type; invalid op rejected by schema.
- Golden-image criteria deferred to EPIC-04 (preview vs export parity, §9).

## Acceptance criteria
- [x] 00-011: Spec v1 + JSON Schema published in `docs/` with examples for all v1 ops.
- [ ] Schema round-trips: a serialized recipe re-parses to identical ops.
