# task: add tools upload dialog

**Feature:** [FEAT-08-02-export-plugin-system](FEAT-08-02-export-plugin-system.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
Tools → Upload dialog for batch upload with size choice and progress/cancellation (PLAN §2 Tools menu "Upload: upload to web services via export plugins"; feature acceptance). Drives any registered plugin from 08-007.

## Functional specs
- Opens with the current selection/Photo Tray; user picks target plugin (and account/destination), album (create-or-pick where supported), and size (original or resized).
- Per-file progress, overall progress, and cancel; cancel finishes the current file cleanly and records what was uploaded.
- Errors per file are shown and retryable without redoing succeeded files.
- Works with every registered plugin through the standard job shape.

## Functional-adjacent edge cases
- Plugin connection lost mid-batch → batch pauses with retry, not silent partial failure.
- Uploading while library scan runs must not block the UI (job queue priorities).

## Technical specs
- UI (Svelte): one dialog component, plugin/target list from the registry's declared capabilities; progress via Tauri events from the upload job.
- Upload batch runs as a job-queue task (PLAN §3 rule 4) so it's cancellable and throttled.
- Resized outputs produced by the EPIC-04 export pipeline into temp files before upload.

## Testing criteria (TDD)
- E2E: select 3 photos → Tools → Upload → local-folder plugin → files appear at destination with chosen size.
- Integration: cancel mid-batch → succeeded files remain, partial file cleaned up, dialog state consistent.

## Acceptance criteria
- [ ] 08-013: Tools → Upload dialog batch-uploads with size choice, progress, and cancellation across all registered plugins.
- [ ] 08-013: Per-file errors are retryable without redoing succeeded files.
