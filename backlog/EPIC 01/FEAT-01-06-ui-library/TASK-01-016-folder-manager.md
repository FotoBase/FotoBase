# task: build folder manager dialog

**Feature:** [FEAT-01-06-ui-library](FEAT-01-06-ui-library.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in review (2026-10-21)

## Description
Implement the Folder Manager dialog per PLAN §2A (Picasa's Folder Manager): per folder Scan Once / Scan Always / Remove from catalog, driving the scanner (01-006) and watcher (01-007). The library mirrors real disk folders; nothing is copied.

## Functional specs
- Dialog lists known folders with their current mode; user can set Scan Once, Scan Always (watched), or Remove.
- Scan Always folders get the notify watcher; Remove drops the folder from the catalog only — files stay on disk, with confirmation.
- Library reflects mode changes immediately (new folder scanned on apply).

## Technical specs
- Modes persisted with the folder rows in `catalog`; applying modes triggers scanner/watcher registration via Tauri commands.
- Face-detection toggle in the dialog is a P3 placeholder (§2 Tools menu note) — skeleton only.

## Testing criteria (TDD)
- Integration: mark a folder Scan Always ⇒ create a file inside ⇒ it appears without manual scan.
- E2E: Remove a folder ⇒ confirm dialog ⇒ folder's photos leave the grid but files remain on disk.

## Acceptance criteria
- [x] 01-016: Folder Manager applies Scan Once / Scan Always / Remove per §2A; library updates immediately.
