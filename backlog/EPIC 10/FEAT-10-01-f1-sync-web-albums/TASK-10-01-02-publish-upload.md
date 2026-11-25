# task: publish web album with incremental re-upload

**Feature:** [FEAT-10-01-f1-sync-web-albums](FEAT-10-01-f1-sync-web-albums.md) · **Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** in review (2026-11-25)

## Description
Publish the generated gallery (task 01) to static-hosting targets under an unguessable URL, with incremental re-publish so it behaves like "Sync to Web" (PLAN §7 F1 layer 1). This task supports local folder, S3-compatible storage (including R2), and WebDAV/Nextcloud.

## Functional specs
- Publish an album to a chosen supported static-hosting target; URL contains an unguessable token.
- Re-publishing after changes uploads only changed files.
- Removing a photo from the album removes it from the published site on next publish.

## Technical specs
- Reuse the EPIC-08 export plugin registry and contract-test suite (upload, resume, retry, album mapping).
- Use a random 128-bit token in the path prefix, a caller-supplied public base URL, and a content-hash manifest for incremental upload and stale-file removal.
- Photo-storage-only services and GitHub Pages are outside this task's supported target set.

## Testing criteria (TDD)
- integration: publish twice with one changed photo; assert second publish uploads only the changed assets.
- unit: URL token generation has ≥128 bits of entropy; manifest diff logic covered by table test.

## Acceptance criteria
- [x] F1-002: Album publishes to supported static-hosting targets under an unguessable URL; re-publish is incremental.
