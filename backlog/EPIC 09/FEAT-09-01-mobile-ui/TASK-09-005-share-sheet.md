# task: integrate platform share sheet

**Feature:** [FEAT-09-01-mobile-ui](FEAT-09-01-mobile-ui.md) · **Epic:** [EPIC-09 — Mobile](../EPIC-09.md) · **Status:** todo

## Description
Add share-sheet integration so a photo can be shared to platform targets (Messages, Mail, social apps) on iOS and Android (PLAN §6 Phase 5 "share"; epic acceptance "share on iOS + Android").

## Functional specs
- Share action available from grid (multi-select) and viewer (single photo).
- Opens the native OS share sheet with the selected image(s) attached.
- Multi-select shares multiple images in one sheet invocation.

## Technical specs
- Tauri v2 share/plugin or platform API via the mobile shell; no custom share UI.
- Shares the current rendered image file (or a resized copy if memory requires).

## Testing criteria (TDD)
- e2e (on-device/emulator): share action opens the OS share sheet with the image present.
- unit: multi-select produces the correct list of files handed to the platform share API.

## Acceptance criteria
- [ ] 09-005: Native share sheet opens from viewer and grid on iOS and Android.
- [ ] Selected image(s) are attached to the share payload.
- [ ] Multi-select share works for more than one photo.
