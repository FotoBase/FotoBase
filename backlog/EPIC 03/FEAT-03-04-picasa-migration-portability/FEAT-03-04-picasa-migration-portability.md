# feat: Picasa migration & portability

**Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** planned

## Acceptance criteria

- Picasa.ini ingest on first scan converts legacy stars, albums, and faces to XMP; a real Picasa library test case migrates with no loss.
- Portable export (JSON/CSV) covers albums, tags, recipes; a user can verify their data outside the app (anti-lock-in acceptance, §5).
- Multiple catalogs switch cleanly (work/personal) with no cross-contamination of caches or watcher state.

- [x] `03-022` (M) Picasa.ini ingest on first scan: legacy stars, albums, faces → XMP ("your old library, restored").
- [x] `03-023` (M) Portable export: albums + tags + recipes to JSON/CSV (§5 anti-lock-in).
- [x] `03-024` (M) Optional multiple catalogs (work/personal); catalog switcher UI.

## Tasks

- [x] [TASK-03-022-picasa-ini-ingest.md](TASK-03-022-picasa-ini-ingest.md) — picasa.ini ingest on first scan
- [x] [TASK-03-023-portable-export.md](TASK-03-023-portable-export.md) — portable export of user data to JSON/CSV
- [ ] [TASK-03-024-multiple-catalogs.md](TASK-03-024-multiple-catalogs.md) — multiple catalogs and catalog switcher
