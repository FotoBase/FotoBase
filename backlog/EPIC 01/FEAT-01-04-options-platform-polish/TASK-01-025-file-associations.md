# task: add file-association registration option

**Feature:** [FEAT-01-04-options-platform-polish](FEAT-01-04-options-platform-polish.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in review (2026-10-22)

## Description
Implement the file-association option (PLAN §2M "File associations: default viewer for image types", P1): register Fotobase as a viewer for selected image types, and unregister cleanly. P1 scope is registration only — the viewer itself is 01-019.

## Functional specs
- Options UI lists supported image extensions with per-type registration.
- Registering makes the OS open those files in Fotobase; un-registering restores prior state cleanly (no orphans).
- Opening a file from the OS launches Fotobase and shows it in the viewer.

## Technical specs
- Per-OS registration: Windows registry / Default Programs API, macOS LaunchServices (LSHandler plist), Linux `mimeapps.list` + desktop file (Tauri v2 deep-link/association APIs where they cover it).
- File-open event routes to the 01-019 viewer via Tauri events.

## Testing criteria (TDD)
- E2E per OS: register an extension, invoke an OS "open file" ⇒ Fotobase opens showing the photo; unregister ⇒ the OS mapping is removed.

## Acceptance criteria
- [x] 01-025: File-association option registers/un-registers image types per OS; OS-opened files land in the viewer.
