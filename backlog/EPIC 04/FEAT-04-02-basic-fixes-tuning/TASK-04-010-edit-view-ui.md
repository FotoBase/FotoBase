# task: build edit view ui

**Feature:** [FEAT-04-02-basic-fixes-tuning](FEAT-04-02-basic-fixes-tuning.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** todo

## Description
The single-photo edit view: large preview, edit tabs (Basic Fixes, Tuning, Effects), filmstrip, and live preview (PLAN §2D "Edit view — single photo with edit tabs and filmstrip"; §6 P2b).

## Functional specs
- Opening a photo in edit view shows tabs + filmstrip of siblings; arrow keys/photo click navigate.
- Every edit control updates the preview live at interactive speeds.
- The original file is never modified in the edit view (only on explicit Save, FEAT-04-06).

## Technical specs
- Svelte 5 component in `app/src`; preview served via `preview://` URI scheme (§3 rule 3) — no image bytes over IPC.
- Edit ops invoke Tauri commands that update the recipe and request a re-render; renders at screen resolution in the job queue with visible-item priority (§3 rule 4).

## Testing criteria (TDD)
- E2e (WebdriverIO + tauri-driver): open edit view, apply an op from each tab, verify preview updates and undo works.
- Integration: edit-session checksum of the original before/after is identical (§9 Data safety).

## Acceptance criteria
- [ ] 04-010: Edit view shows single photo, edit tabs, filmstrip, live preview.
- [ ] 04-010: Originals never modified during editing (checksum-verified).
