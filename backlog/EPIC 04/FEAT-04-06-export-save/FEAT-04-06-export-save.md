# feat: export & save

**Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** planned

## Acceptance criteria

- Export dialog bakes edits at full resolution with resize/quality/numbering/watermark options; exported colors are color-managed.
- "Save" moves the original to sibling `.originals/`, writes edited pixels to the original path, records the stack; Revert restores the original byte-identical.
- "Save as…" / "Save a copy" never touch the original (checksum-verified in tests).
- Recipe ops produce identical results at preview and export resolution (golden images, §9).

- [ ] `04-015` (L) Export dialog: original or custom resize (H/V pixel limit, slider + input), quality presets (Auto/Normal/Maximum/Minimum/Custom %), numbered filenames, text watermark; edits baked in.
- [ ] `04-016` (M) "Save" per §F spec: write edited pixels to original path, original → sibling `.originals/`, record as stacked version; Revert restores from `.originals/`.
- [ ] `04-017` (M) "Save as…" and "Save a copy" (never touch the original).
- [ ] `04-018` (M) Export engine runs recipe at full resolution (shared with 04-001); color-managed output.

## Tasks

- [ ] [TASK-04-015-export-dialog.md](TASK-04-015-export-dialog.md) — build export dialog
- [ ] [TASK-04-016-save-originals.md](TASK-04-016-save-originals.md) — implement save with originals backup
- [ ] [TASK-04-017-save-as-copy.md](TASK-04-017-save-as-copy.md) — implement save-as and save-a-copy
- [ ] [TASK-04-018-fullres-export-engine.md](TASK-04-018-fullres-export-engine.md) — build full-resolution color-managed export engine
