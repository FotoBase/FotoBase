# task: build backup manager with incremental sets

**Feature:** [FEAT-08-01-backup](FEAT-08-01-backup.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
Backup Manager (Tools menu) creating incremental backup sets of catalog + sidecars + settings to an external location (PLAN §2 Tools menu Backup Manager; §6 P4 "Incremental backup + restore"). Backs up user data per the PLAN §5 authority split — sidecars and settings are the crown jewels; the catalog DB rides along for speed.

## Functional specs
- User picks a destination (external drive/path); backup runs with progress and a schedule option.
- First backup is full; subsequent backups are incremental (only changed files/new sidecars).
- Backup sets are self-describing (manifest) so any set can be restored without the app state that created it.
- Destination full/unplugged mid-backup → clean stop with clear error, no corrupt set.

## Technical specs
- Backup set layout: manifest (JSON: version, date, file list with hashes) + content-addressed or path-mirrored files; catalog DB file, all XMP sidecars, settings files.
- Incremental: manifest diff against the previous set's manifest (size+mtime+hash, per PLAN §5 incremental-rebuild logic).
- Runs as a cancellable job-queue task; hashing throttled on battery.
- External location detection: verify writability before starting.

## Testing criteria (TD)
- Integration: backup a fixture library twice with one sidecar changed → set 2 contains only the delta + manifest referencing set 1.
- Integration: unplug simulation (destination becomes read-only mid-run) → no partial manifest written.

## Acceptance criteria
- [ ] 08-014: Backup Manager creates incremental backup sets (catalog + sidecars + settings) to an external location with progress and scheduling.
- [ ] 08-014: Incremental sets contain only the delta; sets are self-describing.
