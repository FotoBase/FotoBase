# feat: export plugin system

**Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** planned

## Acceptance criteria

- Plugin trait + registry; every plugin (local folder, Immich, S3, WebDAV/Nextcloud, Google Photos) passes the shared contract suite: upload, resume, retry, idempotency, album mapping (epic acceptance).
- Interrupted uploads resume without duplicates; retried failures don't produce partial albums.
- Google Photos is upload-only per API limits; Immich plugin's design leaves room for future read-back.
- Tools → Upload dialog batch-uploads with size choice and progress/cancellation.

- [ ] `08-007` (M) Export plugin trait + registry; shared contract test suite (upload, resume, retry, idempotency, album mapping).
- [ ] `08-008` (M) Plugin: local folder.
- [ ] `08-009` (L) Plugin: Immich (API; design for future read-back/"cloud mirror" — §1 strategic note).
- [ ] `08-010` (M) Plugin: S3-compatible.
- [ ] `08-011` (M) Plugin: WebDAV/Nextcloud.
- [ ] `08-012` (M) Plugin: Google Photos (upload-only, §10 API restriction).
- [ ] `08-013` (M) Tools → Upload dialog (batch upload with size choice).

## Tasks

- [ ] [TASK-08-007-plugin-trait-registry.md](TASK-08-007-plugin-trait-registry.md) — export plugin trait, registry, contract tests
- [ ] [TASK-08-008-plugin-local-folder.md](TASK-08-008-plugin-local-folder.md) — local folder plugin
- [ ] [TASK-08-009-plugin-immich.md](TASK-08-009-plugin-immich.md) — Immich plugin
- [ ] [TASK-08-010-plugin-s3.md](TASK-08-010-plugin-s3.md) — S3-compatible plugin
- [ ] [TASK-08-011-plugin-webdav.md](TASK-08-011-plugin-webdav.md) — WebDAV/Nextcloud plugin
- [ ] [TASK-08-012-plugin-google-photos.md](TASK-08-012-plugin-google-photos.md) — Google Photos plugin (upload-only)
- [ ] [TASK-08-013-upload-dialog.md](TASK-08-013-upload-dialog.md) — Tools → Upload dialog
