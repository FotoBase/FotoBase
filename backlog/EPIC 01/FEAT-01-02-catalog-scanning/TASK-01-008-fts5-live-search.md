# task: add FTS5 live search over filenames and folders

**Feature:** [FEAT-01-02-catalog-scanning](FEAT-01-02-catalog-scanning.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in review (2026-10-21)

## Description
Implement FTS5 full-text search over filename + folder name (PLAN §5 `files_fts`, P1 scope per §2C: live search filters filenames/folders; metadata search arrives in P2) with a query API the UI can call as you type.

## Functional specs
- Typing in the search bar (01-018) filters the library live across filenames and folder names.
- On the 50k fixture, results return with no UI jank.
- FTS index stays in sync as the scanner adds/removes files.

## Technical specs
- SQLite FTS5 virtual table `files_fts` over filename + folder (§5); P2 extends it to caption/tags/people/place.
- Query API exposed as a Tauri command (§3) returning file ids; UI does the rendering.

## Testing criteria (TDD)
- Unit: given a catalog with "beach.jpg" in "/photos/2024", queries "beach" and "2024" both return it; a query for "xyzzy" returns nothing.
- Integration: scan a new file ⇒ it is immediately searchable.

## Acceptance criteria
- [x] 01-008: FTS5 over filename + folder name with a live query API, kept in sync with scans.
