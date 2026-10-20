# feat: backup

**Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** planned

## Acceptance criteria

- Backup Manager creates incremental backup sets (catalog + sidecars + settings) to an external location with progress and scheduling.
- Backup → wipe → restore reproduces library and sidecars byte-identical (§9 acceptance test).
- Restore validates set integrity before writing anything.

- [ ] `08-014` (L) Backup Manager: incremental backup sets of catalog + sidecars + settings to external location.
- [ ] `08-015` (M) Restore from backup sets; byte-identical restore acceptance test (§9).

## Tasks

- [ ] [TASK-08-014-backup-manager.md](TASK-08-014-backup-manager.md) — backup manager with incremental sets
- [ ] [TASK-08-015-restore-byte-identical.md](TASK-08-015-restore-byte-identical.md) — restore from backup sets
