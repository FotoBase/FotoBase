# task: add notify watcher for scan-always folders

**Feature:** [FEAT-01-02-catalog-scanning](FEAT-01-02-catalog-scanning.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in review (2026-10-21)

## Description
Implement the `notify`-based watcher (PLAN §4) for "Scan Always" folders (§2A): file events are debounced and fed into the job queue so new/changed/deleted files are picked up live. Local folders only in P1 — network paths get a polling watcher in P1.5 (§4).

## Functional specs
- Creating, modifying, or deleting a file in a Scan Always folder updates the library without a manual rescan.
- Debounced: editor save-storms and temp files don't trigger per-event scans.
- No file is scanned twice concurrently.

## Technical specs
- `notify` crate; debounce window tuned (hundreds of ms); events map to scanner incremental jobs in the `jobs` queue.
- Watcher lifecycle tied to the Folder Manager's Scan Always list (01-016).

## Testing criteria (TDD)
- Integration: write then rename-in a file in a watched folder ⇒ exactly one catalog insert after debounce.
- Integration: two rapid modifications coalesce into one rescan job.

## Acceptance criteria
- [x] 01-007: Watcher on Scan Always folders with debounced events into the job queue; no double-scans.
