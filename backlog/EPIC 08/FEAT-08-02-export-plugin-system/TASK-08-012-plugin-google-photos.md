# task: add google photos export plugin

**Feature:** [FEAT-08-02-export-plugin-system](FEAT-08-02-export-plugin-system.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
Google Photos upload plugin, upload-only per API restrictions (PLAN §10 risk: "since 2025, apps can only read back their own uploads"; §6 P4 "Google Photos (upload-only)").

## Functional specs
- OAuth flow for Google account consent (photos scope); tokens stored locally, refresh handled.
- Uploads to a Google Photos album with size choice; resumable per the contract suite.
- The UI states clearly it's one-way — no sync/read-back.
- Album mapping: create-or-find album by name idempotently.

## Technical specs
- Google Photos API v1 (`mediaItems.upload` two-step: raw upload → media item create; resumable via the upload protocol's chunking).
- Idempotency: our content-key ledger (local record of uploaded content keys per album) since the API gives no dedupe; retry detection consults it before re-uploading.
- OAuth: system browser flow, token in OS keychain/credential store.
- Contract suite: recorded-HTTP mock (no live Google in CI); a manual/live test checklist for release.

## Testing criteria (TDD)
- Integration: contract suite against the recorded-API mock — upload/resume/retry/album mapping.
- Unit: content-key ledger prevents duplicate upload on retry.

## Acceptance criteria
- [ ] 08-012: Google Photos plugin passes the contract suite, upload-only per §10 API restriction.
- [ ] 08-012: Duplicate uploads are prevented via the local content-key ledger.
