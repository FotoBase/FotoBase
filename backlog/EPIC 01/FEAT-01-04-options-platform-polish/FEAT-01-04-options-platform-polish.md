# feat: options & platform polish

**Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** planned

## Acceptance criteria

- Options dialog persists all settings; update modes (auto/check-but-ask/off) behave as labeled; "clear cache files" empties the thumbnail cache safely.
- Rebuild catalog re-scans watched folders with a progress indicator and a responsive UI throughout.
- Dark mode + HiDPI render correctly on all three OSes; documented keyboard shortcuts work for every core flow.
- File-association option registers Fotobase as default viewer for selected image types (and un-registers cleanly).

- [x] `01-022` (M) Options dialog (General): language, automatic updates (auto/check-but-ask/off), clear cache files, show tooltips.
- [x] `01-023` (M) Rebuild catalog command (from watched folders; progress-reported).
- [x] `01-024` (M) Dark mode + HiDPI on all three OSes; keyboard shortcuts for core flows.
- [ ] `01-025` (S) File-association option: register as default viewer for image types (P1 scope: registration only, viewer exists via 01-019).

## Tasks

- [x] [TASK-01-022-options-dialog.md](TASK-01-022-options-dialog.md) — build options dialog (general tab)
- [x] [TASK-01-023-rebuild-catalog.md](TASK-01-023-rebuild-catalog.md) — add rebuild catalog command with progress
- [x] [TASK-01-024-dark-mode-hidpi-shortcuts.md](TASK-01-024-dark-mode-hidpi-shortcuts.md) — polish dark mode, hidpi, and keyboard shortcuts
- [ ] [TASK-01-025-file-associations.md](TASK-01-025-file-associations.md) — add file-association registration option
