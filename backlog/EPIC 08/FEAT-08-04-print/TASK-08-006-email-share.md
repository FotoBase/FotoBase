# task: add email via os share sheet and mailto

**Feature:** [FEAT-08-04-print](FEAT-08-04-print.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
Email selected photos with auto-resized attachments via the OS share sheet and a `mailto` fallback (PLAN §2 §J Email P4: "OS share sheet + `mailto`"). Reuses the EPIC-04 export pipeline for resizing.

## Functional specs
- "Email" action on a selection (or Photo Tray) resizes images to an emailable size (user-confirmable, sensible default) with edits baked in, then opens the OS share/mail flow.
- On OSes/flows without a share sheet, falls back to `mailto:` with the resized files attached (or revealed) via the OS mail client.
- Originals are never touched; resized copies go to a temp location.
- Works for multi-selection.

## Technical specs
- Tauri v2: `tauri-plugin-share`/OS share APIs where available; `mailto:` URL open otherwise. Attachments: platform-dependent — write resized JPEGs to temp dir; where the OS mail client can't take attachments via URL, open the temp folder.
- Resizing: EPIC-04 export pipeline (quality preset "Normal"-class, max dimension default ~1600px) into temp files.

## Testing criteria (TDD)
- Unit: email export of a 6000px photo produces a temp JPEG ≤ the max dimension.
- E2E (per OS): invoking Email opens a share sheet or mail client without error; temp files exist and are the resized size.

## Acceptance criteria
- [ ] 08-006: Email via OS share sheet / `mailto:` attaches auto-resized images, originals untouched.
- [ ] 08-006: Works for multi-selection with a graceful fallback on all three OSes.
