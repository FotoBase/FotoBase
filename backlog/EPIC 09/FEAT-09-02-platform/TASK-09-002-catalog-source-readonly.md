# task: implement read-only-safe single-catalog source

**Feature:** [FEAT-09-02-platform](FEAT-09-02-platform.md) · **Epic:** [EPIC-09 — Mobile](../EPIC-09.md) · **Status:** todo

## Description
Enable the mobile app to open a single catalog source — a NAS path or a Syncthing/iCloud-synced folder — with a read-only-safe open path (PLAN §6 Phase 5: decoupled from F1.2; concurrent external writes must never corrupt state, and the app never writes to the source unguarded).

## Functional specs
- User can point the app at a catalog source (remote/synced folder) and browse it.
- Concurrent external writes (sync client updating files mid-scan) never corrupt app state; the app re-scans or surfaces errors gracefully.
- The app never writes to the source without an explicit user action.
- Works fully offline against a locally synced copy.

## Technical specs
- Reuse the EPIC-01 catalog scanner with a mobile-capable path layer (SMB/WebDAV or local synced folder).
- Writes (star/tag from FEAT-09-01 task 02) go through the shared safe-write path; treat partially-synced files atomically (skip files appearing mid-write).

## Testing criteria (TDD)
- integration: scan a fixture folder while a background process mutates it; catalog ends consistent, no crash.
- unit: files that change size/mtime mid-scan are skipped and re-queued, not partially indexed.

## Acceptance criteria
- [ ] 09-002: App opens a NAS or Syncthing/iCloud-synced catalog source and browses it.
- [ ] Concurrent external writes never corrupt catalog state (tested).
- [ ] No unguarded writes to the source.
