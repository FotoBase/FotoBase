# task: add screen capture folder

**Feature:** [FEAT-08-03-misc-output](FEAT-08-03-misc-output.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
PrtScn key captures the screen into a "Screen Captures" library folder (PLAN §2 §A Screen capture P4: "PrtScn key saved screenshots directly into a 'Screen Captures' library folder").

## Functional specs
- While the app runs (background/tray included), pressing PrtScn (Print Screen; macOS uses its own system shortcuts) saves the screenshot into the catalog's "Screen Captures" folder with a timestamped name.
- The folder is a normal watched library folder — captures appear in the grid immediately via the existing watcher.
- Feature is opt-in (global hotkey registration asks consent; users may not want the app grabbing PrtScn).
- Multi-monitor: all displays or the active one, per a simple setting.

## Technical specs
- Global hotkey registration via Tauri (global-shortcut plugin); capture via OS screen-capture APIs (Windows GDI/DXGI, macOS ScreenCaptureKit-class, Linux XDG desktop portal) — no screenshots when the app has no capture permission (macOS prompts once).
- Writes a PNG into `<catalog>/Screen Captures/` and registers the folder as scanned-always if not present.
- Tray-icon background running (EPIC-03/09 platform work) is what keeps the hotkey alive when the window is closed.

## Testing criteria (TDD)
- Integration: simulated hotkey event → PNG file created in the Screen Captures folder with timestamped name; watcher picks it up (grid shows it).
- Unit: opt-in disabled → no hotkey registered, no capture.

## Acceptance criteria
- [ ] 08-019: PrtScn saves screenshots into the "Screen Captures" library folder, opt-in, appearing in the grid via the watcher.
- [ ] 08-019: No capture happens without consent/permission.
