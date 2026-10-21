# feat: catalog & scanning

**Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** planned

## Acceptance criteria

- 50k-photo initial scan populates the catalog in the background; schema matches §5 (minus AI tables); migrations run forward-only and are tested.
- Second scan of an unchanged library is a fast no-op (mtime+size); changed/added/deleted files are detected incrementally.
- Watched ("Scan Always") folders pick up new/changed/deleted files via debounced watcher events; no file is scanned twice concurrently.
- Live search over filenames/folders returns results as you type on the 50k fixture with no UI jank.
- SQLite runs WAL with startup `integrity_check` and rolling backups; killing the process mid-scan never corrupts the catalog.

- [x] `01-005` (M) `catalog` crate: SQLite schema (§5 tables minus AI ones), migrations, WAL, startup `integrity_check`, rolling backups.
- [x] `01-006` (M) `scanner` crate: folder walk, mtime+size change detection, initial scan populating catalog; incremental by default.
- [x] `01-007` (M) `notify` watcher for "Scan Always" folders; watcher events debounced into the job queue.
- [ ] `01-008` (M) FTS5 over filename + folder name; live search query API.

## Tasks

- [x] [TASK-01-005-catalog-crate.md](TASK-01-005-catalog-crate.md) — build catalog crate — schema, migrations, robustness
- [x] [TASK-01-006-scanner-crate.md](TASK-01-006-scanner-crate.md) — build scanner crate — walk, change detection, initial scan
- [x] [TASK-01-007-notify-watcher.md](TASK-01-007-notify-watcher.md) — add notify watcher for scan-always folders
- [ ] [TASK-01-008-fts5-live-search.md](TASK-01-008-fts5-live-search.md) — add FTS5 live search over filenames and folders
