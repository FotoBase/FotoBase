# task: build scanner crate — walk, change detection, initial scan

**Feature:** [FEAT-01-02-catalog-scanning](FEAT-01-02-catalog-scanning.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in progress (2026-10-21)

## Description
Implement the `scanner` crate per PLAN §3/§5: folder walk over watched folders, mtime+size change detection, initial scan populating the catalog, incremental by default (§5: full rebuilds are explicit maintenance actions).

## Functional specs
- Initial scan of a 50k-photo library runs in the background and populates the catalog without UI jank.
- Second scan of an unchanged library is a fast no-op (mtime+size skip).
- Added, changed (mtime or size differ), and deleted files are detected incrementally.

## Technical specs
- Walk uses the `catalog` crate's `folders`/`files` tables; content hash computed lazily (§10: cache by mtime+size, hash lazily).
- Scan work feeds the job queue (§3 rule 4) so it stays bounded and cancellable.

## Testing criteria (TDD)
- Integration: given the 1k fixture, scan populates N rows; rescan without changes performs no file re-reads (assert skipped count).
- Integration: touch/modify/delete a file between scans ⇒ exactly that file is updated/removed.

## Acceptance criteria
- [x] 01-006: `scanner` crate walks folders, detects changes by mtime+size, populates catalog on initial scan, incremental by default.
