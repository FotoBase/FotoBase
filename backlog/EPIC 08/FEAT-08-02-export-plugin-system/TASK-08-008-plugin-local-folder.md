# task: add local folder export plugin

**Feature:** [FEAT-08-02-export-plugin-system](FEAT-08-02-export-plugin-system.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
The first concrete plugin: export to a local folder, with album mapping as subfolders (PLAN §6 P4: "plugins: local folder"). Doubles as the reference implementation proving the 08-007 trait and contract suite.

## Functional specs
- User picks a destination folder; photos export there (original or resized per upload options), albums map to subfolders.
- Re-running the same upload doesn't duplicate files (idempotent overwrite/skip).
- Interrupted export resumes by skipping files already fully written.

## Technical specs
- Implements the `export` crate `ExportPlugin` trait; "remote endpoint" is the filesystem.
- Atomic writes (temp file + rename) so interrupted files aren't half-written.
- Album mapping: album name → subfolder; passes the 08-007 contract suite with a temp-dir sandbox.

## Testing criteria (TDD)
- Integration: contract suite against a temp dir — upload 5 files, kill mid-batch, resume → 5 files, no partials.
- Unit: album mapping creates nested subfolders with sanitized names.

## Acceptance criteria
- [ ] 08-008: Local folder plugin passes the full contract suite with album→subfolder mapping.
- [ ] 08-008: Exports are atomic and resumable without duplicates.
