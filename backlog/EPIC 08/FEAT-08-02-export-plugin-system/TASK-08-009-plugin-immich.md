# task: add immich export plugin

**Feature:** [FEAT-08-02-export-plugin-system](FEAT-08-02-export-plugin-system.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
Immich plugin via the Immich API, designed so a future read-back/"cloud mirror" view is possible (PLAN §1 strategic note: a lightweight Immich desktop client is potentially a bigger market than standalone Fotobase).

## Functional specs
- User configures server URL + API key; connection is validated with a clear error on failure.
- Uploads photos to a chosen Immich album with size choice; resumable and idempotent per the contract suite.
- No data is read back in v1 — but the plugin trait surface (list/exists queries) is designed so read-back can be added without breaking the trait.
- Works fully offline-configurable: no telemetry, server URL never leaves the machine except to the server itself.

## Technical specs
- Immich API: `/api/albums`, upload endpoint with multipart; API key header auth. HTTP via `reqwest` in the plugin.
- Idempotency: Immich's duplicate detection + our content-key; album creation idempotent by name lookup before create.
- Trait notes: implement the optional read-back queries as unimplemented-or-simple so the future mirror needs no trait change.
- Contract suite runs against a containerized Immich instance in CI (or a recorded-API mock if the container is too heavy).

## Testing criteria (TDD)
- Integration: contract suite against a local Immich container — upload/resume/retry/album mapping all pass.
- Integration: uploading the same file twice produces one asset (idempotency).
- Unit: URL/key validation rejects malformed config before any network call.

## Acceptance criteria
- [ ] 08-009: Immich plugin passes the contract suite and leaves room in its design for future read-back/"cloud mirror" (§1 strategic note).
- [ ] 08-009: Connection errors are surfaced clearly; duplicate uploads are prevented.
