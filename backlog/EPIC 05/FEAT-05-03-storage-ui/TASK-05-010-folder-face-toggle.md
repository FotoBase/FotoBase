# task: add per-folder face-detection toggle

**Feature:** [FEAT-05-03-storage-ui](FEAT-05-03-storage-ui.md) · **Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** todo

## Description
Per-folder face-detection toggle in the Folder Manager (PLAN §2 Tools menu "Folder Manager — face-detection toggle"; §6 P3 People).

## Functional specs
- Folder Manager shows a face-detection checkbox per folder.
- Disabled folders: no face-detection jobs scheduled; existing faces left alone.
- Toggling on later triggers detection for that folder.

## Technical specs
- Per-folder setting stored in catalog `folders` table (scan-state/derived domain, §5).
- Consumed by the face job scheduler from 05-002.

## Testing criteria (TDD)
- Integration: folder toggled off → full-library face pass skips it; toggled on → pass processes it.
- E2e: toggle visible and functional in Folder Manager dialog.

## Acceptance criteria
- [ ] 05-010: Per-folder face-detection toggle in Folder Manager controls scheduling.
