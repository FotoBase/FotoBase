# task: implement file-menu basics and folder commands

**Feature:** [FEAT-01-07-ui-viewer-info](FEAT-01-07-ui-viewer-info.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** todo

## Description
Implement the P1 File-menu and folder-context commands per PLAN §2 File menu / Folder context menu: Locate on disk, Delete from disk (trash + confirmation), Exit; folder "Remove from Fotobase" (catalog only) and Refresh thumbnails.

## Functional specs
- Locate on disk reveals the photo (or folder) in the OS file manager.
- Delete from disk moves the photo to the OS trash after a confirmation dialog — never a permanent delete.
- Remove from Fotobase removes the folder from the catalog only, files untouched, with confirmation.
- Refresh thumbnails re-queues renders for the folder's photos through the job queue.
- Exit quits the app cleanly (jobs drained or persisted safely).

## Technical specs
- Reveal via `opener`/Tauri shell APIs; trash via `trash` crate or OS API; Delete also drops the catalog row; originals otherwise read-only (§3 rule 1).
- Refresh throttles into the `jobs` queue at background priority.

## Testing criteria (TDD)
- E2E: Locate on disk opens the file manager with the file selected (per-OS smoke).
- Integration: Delete a fixture photo ⇒ file is in OS trash, catalog row gone, confirmation was required.
- Integration: Remove folder ⇒ files still on disk, rows gone; Refresh thumbnails ⇒ re-render jobs enqueued.

## Acceptance criteria
- [ ] 01-021: Locate on disk, Delete from disk (trash + confirm), Exit, folder Remove from Fotobase (catalog only), Refresh thumbnails — all wired.
