# task: add export plugin trait, registry, and contract tests

**Feature:** [FEAT-08-02-export-plugin-system](FEAT-08-02-export-plugin-system.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
The export plugin trait + registry in the `export` crate (PLAN §3 module layout) and the shared contract test suite every plugin must pass: upload, resume, retry, idempotency, album mapping (PLAN §6 P4 acceptance, §9 "Shared contract tests per plugin"). Foundation for all plugins in this feature and the F5 ecosystem.

## Functional specs
- Plugins register with metadata (name, capabilities: album support, size choice) and appear wherever upload targets are listed.
- All uploads are resumable: an interrupted upload resumes without duplicates (feature acceptance).
- Retried failures don't produce partial albums (feature acceptance) — album creation is idempotent per job.
- A plugin that fails the contract suite cannot ship.

## Technical specs
- `export` crate: `trait ExportPlugin { fn upload(&mut self, job: UploadJob) -> …; fn list_albums/create_album; }`-shape (async, tokio); registry keyed by plugin id.
- Upload jobs: batch of files + resize/quality options (reuse EPIC-04 export pipeline for resized outputs), target album, per-file progress events to the UI.
- Contract suite: Rust test harness with a mock plugin; asserts upload/resume/retry/idempotency/album-mapping semantics; each plugin runs the same suite against a sandboxed local endpoint.
- Idempotency mechanism: stable content-key per file so retries are recognized server-side.

## Testing criteria (TDD)
- Integration (write the contract suite first): mock plugin — kill mid-upload → resume → no duplicate files; fail during album creation → retry → exactly one album.
- Unit: registry lists registered plugins with declared capabilities.
- Integration: content-key retry detection dedupes a re-uploaded identical file.

## Acceptance criteria
- [ ] 08-007: Export plugin trait + registry exist and every plugin passes the shared contract suite (upload, resume, retry, idempotency, album mapping).
- [ ] 08-007: Interrupted uploads resume without duplicates; retried failures don't produce partial albums.
