# task: tray icon for background watching

**Feature:** [FEAT-03-02-os-integration](FEAT-03-02-os-integration.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in review (2026-10-29)

## Description
System-tray icon showing and controlling background watching, so the watcher keeps running with the main window closed (PLAN §6 P2a deliverable "Tray icon for background watching").

## Functional specs
- Tray icon appears when the app runs; closing the window keeps watching (tray menu: show library, pause/resume watching, quit).
- Scan/watch status is visible from the tray (idle / watching / scanning).
- Quit from tray fully exits (no orphan processes, watcher stops).

## Technical specs
- Tauri v2 tray-icon API; window close event intercepted → hide instead of exit (configurable option; default per P2a).
- Tray menu commands map to existing watcher controls (P1 watcher; polling watcher for NAS from EPIC-02).
- No telemetry/phone-home (§1) — tray status is local only.

## Testing criteria (TDD)
- E2e: close window → process alive; new file in a watched folder is still picked up; tray "quit" ends the process (assert via process check).
- Unit: menu action mapping (pause/resume) toggles watcher state.

## Acceptance criteria
- [x] 03-025: tray icon shows and controls background watching.
- [x] 03-025: watcher runs with the main window closed.
- [x] 03-025: tray quit fully exits the app.
