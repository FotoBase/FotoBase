# task: add external editor round-trip

**Feature:** [FEAT-10-02-f2-advanced-editor](FEAT-10-02-f2-advanced-editor.md) · **Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (deferred)

## Description
"Edit in GIMP / Photoshop / Affinity / darktable": launch the external editor with the photo, and auto-import the saved result as a stacked version, original untouched (PLAN §7 F2 item 1). Depends on the EPIC-04 recipe engine and the EPIC-03 stacking model.

## Functional specs
- User picks an external editor (configured path or OS default association) from the photo context menu.
- On save in the external editor, the result is auto-imported and appears as a stacked version of the original.
- Original file is never modified.

## Technical specs
- Watch the export temp file / use OS file-change notification to detect the external editor's save.
- Stack the result using the EPIC-03 version-stacking data model; no new stacking mechanism.

## Testing criteria (TDD)
- integration (with a script-based fake editor): invoke, save, assert a stacked version appears and the original's hash is unchanged.

## Acceptance criteria
- [ ] F2-001: External editor round-trip auto-imports the result as a stacked version; original untouched.
