# task: build catalog crate — schema, migrations, robustness

**Feature:** [FEAT-01-02-catalog-scanning](FEAT-01-02-catalog-scanning.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** todo

## Description
Implement the `catalog` crate per PLAN §5: the SQLite schema (P1 subset — no AI tables), forward-only migrations, WAL mode, startup `integrity_check`, and automatic rolling backups. The catalog is a rebuildable cache; sidecars remain the source of truth for user data.

## Functional specs
- Fresh install creates the catalog via migrations; upgrades never lose data.
- Startup runs a cheap `integrity_check`; corruption triggers the rebuild-from-sidecars recovery path (§5).
- Daily rolling backups of the catalog file (keep N) to the catalog location.
- Killing the process mid-scan never corrupts the catalog.

## Technical specs
- `rusqlite` (bundled SQLite) + FTS5 (§4); WAL mode.
- Tables per §5 minus AI ones: `folders`, `files` (path, size, mtime, hash, dimensions, taken_at, media_type), `metadata`, `tags`, `file_tags`, `albums`, `album_files`, `files_fts`.
- Migrations forward-only, tested.

## Testing criteria (TDD)
- Unit: migrations run in order on an empty DB and on a previous-version DB; schema matches the §5 table list.
- Integration: kill -9 mid-transaction ⇒ reopen succeeds, `integrity_check` passes.

## Acceptance criteria
- [ ] 01-005: `catalog` crate with §5-minus-AI schema, tested forward-only migrations, WAL, startup `integrity_check`, rolling backups.
