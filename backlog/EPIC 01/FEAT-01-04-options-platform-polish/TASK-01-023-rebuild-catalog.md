# task: add rebuild catalog command with progress

**Feature:** [FEAT-01-04-options-platform-polish](FEAT-01-04-options-platform-polish.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in progress (2026-10-21)

## Description
Implement the Rebuild catalog command (PLAN §2L "Rebuild cache/database", §5 corruption recovery): re-scan watched folders and repopulate the catalog, with a progress indicator and a responsive UI throughout. The catalog is a rebuildable cache — user data lives in sidecars.

## Functional specs
- Triggered from the UI (Tools/maintenance); shows progress (folders/files processed).
- UI stays responsive during rebuild; scans run in the background job queue.
- Rebuild drops and repopulates derived data only; originals and sidecars untouched.

## Technical specs
- Reuses the `scanner` crate (01-006) in full-scan mode; progress events via Tauri events (§3).
- Incremental by default remains the app behavior; full rebuild is this explicit, progress-reported action (§5).

## Testing criteria (TDD)
- Integration: seed catalog with sidecar-backed user data ⇒ rebuild ⇒ user data identical (§9 rebuildability check on the P1 subset).
- E2E: progress indicator advances and UI accepts input during a rebuild of the 1k fixture.

## Acceptance criteria
- [x] 01-023: Rebuild catalog command runs from watched folders with progress reporting and a responsive UI.
