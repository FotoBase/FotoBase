# task: build single-photo viewer with filmstrip and slideshow

**Feature:** [FEAT-01-07-ui-viewer-info](FEAT-01-07-ui-viewer-info.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in review (2026-10-22)

## Description
Implement the single-photo viewer per PLAN §2D (edit-view P1 scope is view-only): opens from the grid with filmstrip navigation, fullscreen mode, and a basic timed slideshow (timing only — music/transitions are P4).

## Functional specs
- Double-click (or Enter) opens the selected photo full-view with a filmstrip of the current folder/selection.
- Keyboard control: arrows navigate, Esc exits, F fullscreen, space toggles slideshow.
- Slideshow: configurable interval, advances through the current selection; pauses on interaction.
- Preloads neighbors so navigation feels instant.

## Technical specs
- Svelte 5 route over the grid's selection; full images served via a `preview://`-style scheme or direct file URL per §3 rule 3 (no IPC bytes).
- Slideshow timer in the frontend; fullscreen via Tauri window API.

## Testing criteria (TDD)
- E2E: open viewer from grid ⇒ filmstrip shows the folder's photos; arrow keys move selection; Esc returns to grid.
- E2E: start slideshow with a 1 s interval on a 3-photo fixture ⇒ all three shown in order.

## Acceptance criteria
- [x] 01-019: Viewer with filmstrip, fullscreen, timed slideshow, full keyboard control.
