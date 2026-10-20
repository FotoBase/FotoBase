# task: add os context menu integration

**Feature:** [FEAT-08-03-misc-output](FEAT-08-03-misc-output.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
OS file-manager context menu entries "View with Fotobase" and "Upload with Fotobase" in Explorer/Finder (PLAN §2 §M Context menu P4; Nautilus included for Linux parity).

## Functional specs
- Right-clicking an image in Explorer/Finder shows "View with…" (opens the photo in the Fotobase viewer) and "Upload…" (opens the app with the Upload dialog pre-loaded with that photo).
- Works when the app is closed (launches it) or running (focuses it).
- Uninstall removes the menu entries.
- macOS: Finder extension/Services; Windows: registry verb; Linux: `.desktop` Nautilus script.

## Technical specs
- Windows: registry `shell` verbs under the image file classes (MSI/NSIS installer writes them).
- macOS: a Finder Sync/Services-based entry or a small `Automator`-style service registered by the app bundle.
- Linux: `~/.local/share/nautilus/scripts/` script or portal-based approach per distro.
- Deep link/single-instance: OS menu passes the file path via the Tauri single-instance plugin argument → routes to viewer or upload dialog.
- Installer/uninstaller owns registration per OS (PLAN §4 Packaging).

## Testing criteria (TDD)
- Integration (per OS in release checklist): context menu appears for JPEG after install; "View with…" opens the photo in the viewer; "Upload…" opens the upload dialog with the file preselected.
- Integration: uninstall removes the entries.

## Acceptance criteria
- [ ] 08-020: OS context menu ("View with…", "Upload") works in Explorer/Finder for image files, launching or focusing the app with the right context.
- [ ] 08-020: Install/uninstall registers and removes the menu entries cleanly.
