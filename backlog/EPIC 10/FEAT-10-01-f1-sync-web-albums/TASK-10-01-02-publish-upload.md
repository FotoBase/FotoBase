# task: publish web album with incremental re-upload

**Feature:** [FEAT-10-01-f1-sync-web-albums](FEAT-10-01-f1-sync-web-albums.md) · **Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (deferred)

## Description
Upload the generated gallery (task 01) to any export target under an unguessable URL, with incremental re-publish so it behaves like "Sync to Web" (PLAN §7 F1 layer 1). Depends on EPIC-08 export plugin targets (S3, R2, GitHub Pages, WebDAV/Nextcloud).

## Functional specs
- Publish an album to a chosen export target; URL contains an unguessable token.
- Re-publishing after changes uploads only changed files.
- Removing a photo from the album removes it from the published site on next publish.

## Technical specs
- Reuse the EPIC-08 export plugin trait and its contract-test suite (upload, resume, retry, album mapping).
- Unguessable URL via random token in the path prefix; incremental diff by content hash manifest.

## Testing criteria (TDD)
- integration: publish twice with one changed photo; assert second publish uploads only the changed assets.
- unit: URL token generation has ≥128 bits of entropy; manifest diff logic covered by table test.

## Acceptance criteria
- [ ] F1-002: Album uploads to any export target under an unguessable URL; re-publish is incremental.
