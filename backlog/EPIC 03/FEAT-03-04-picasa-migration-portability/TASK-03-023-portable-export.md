# task: portable export of user data to JSON/CSV

**Feature:** [FEAT-03-04-picasa-migration-portability](FEAT-03-04-picasa-migration-portability.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** todo

## Description
Built-in portable export of albums, tags, and edit recipes to JSON/CSV, available at any time — the anti-lock-in guarantee (PLAN §5 "Portability guarantee": no data held hostage in the catalog).

## Functional specs
- User picks what to export (albums / tags / recipes) and format (JSON or CSV); export lands in a chosen folder.
- Exported JSON is self-describing and versioned; a user can verify their data outside the app.
- Recipes export per the published versioned open spec (§5: docs + JSON Schema).

## Technical specs
- Export command in `catalog`/`export` layer reading catalog user-data tables; recipe JSON matches the published schema (`version`, `ops[]`).
- CSV for tabular data (filename, album, tags); JSON for full fidelity incl. `fb:Albums` membership and recipes.
- No network, no telemetry (§1); plain file write.

## Testing criteria (TDD)
- Unit: fixture library exports JSON that validates against the published JSON Schema in-repo.
- Integration: export → importable/readable: a parser test round-trips the JSON back into equivalent structures.

## Acceptance criteria
- [ ] 03-023: albums, tags, recipes export to JSON/CSV at any time.
- [ ] 03-023: recipe JSON validates against the versioned schema.
- [ ] 03-023: a user can verify their data outside the app (documented example in repo).
