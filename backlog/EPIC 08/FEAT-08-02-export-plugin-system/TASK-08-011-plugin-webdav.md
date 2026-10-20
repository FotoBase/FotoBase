# task: add webdav/nextcloud export plugin

**Feature:** [FEAT-08-02-export-plugin-system](FEAT-08-02-export-plugin-system.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
Export plugin for WebDAV endpoints (incl. Nextcloud/ownCloud shares) — PLAN §6 P4 plugin list.

## Functional specs
- User configures WebDAV URL, username, password; validation with clear errors.
- Uploads with size choice; albums map to remote directories.
- Resumable: interrupted uploads are re-done per-file atomically; idempotent overwrite by content-key filename.

## Technical specs
- WebDAV client: HTTP verbs `MKCOL`/`PUT`/`PROPFIND` via `reqwest`; digest/basic auth as the server requires.
- Atomic per-file: upload to temp name then `MOVE` to final name (servers supporting it), else size-checked overwrite.
- Contract suite against a local WebDAV test server (e.g. a containerized Nextcloud or a lightweight DAV server).

## Testing criteria (TDD)
- Integration: contract suite vs the DAV test server — upload/resume/retry/album-dir mapping.
- Unit: album name sanitization for remote directory creation.

## Acceptance criteria
- [ ] 08-011: WebDAV/Nextcloud plugin passes the contract suite with directory-based album mapping.
- [ ] 08-011: Interrupted uploads never leave half-written remote files.
