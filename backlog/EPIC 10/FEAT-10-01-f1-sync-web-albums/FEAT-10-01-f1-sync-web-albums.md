# F1 — Sync & web albums

**Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (coarse by design)

## Acceptance criteria

- F1.1: published album is a static site (resized images + tiny JS viewer) that works offline from a file server; re-publish is incremental (only changed assets re-uploaded); URL unguessable.
- F1.2: metadata CRDT merges concurrent edits from two devices without loss; only confirmed face names sync; QR pairing completes without an account; originals not synced by default.
- F1.3 (on demand): self-hosted peer serves private albums with login over axum + SQLite + S3.

## F1.1 Publish web album (after EPIC-08; earliest of the futures, best demo artifact — ship early)

- [ ] `F1-001` (M) Static HTML gallery generator (resized images + tiny JS viewer) for an album.
- [ ] `F1-002` (M) Upload to any export target under unguessable URL; incremental re-publish (= "Sync to Web").

## F1.2 Device sync (after stable XMP model, EPIC-03; before/with EPIC-09 usefulness)

- [ ] `F1-003` (XL) Metadata as CRDT document (Loro/Automerge); only confirmed face names sync.
- [ ] `F1-004` (L) iroh transport (QUIC, relay fallback), QR pairing; originals not synced by default.

## F1.3 Self-hosted server (only on demand)

axum + SQLite + S3 always-on peer, private albums with login.

## Tasks

- [ ] [TASK-10-01-01-gallery-generator.md](TASK-10-01-01-gallery-generator.md) — build static html gallery generator
- [ ] [TASK-10-01-02-publish-upload.md](TASK-10-01-02-publish-upload.md) — publish web album with incremental re-upload
- [ ] [TASK-10-01-03-crdt-metadata.md](TASK-10-01-03-crdt-metadata.md) — implement metadata crdt document
- [ ] [TASK-10-01-04-iroh-sync-transport.md](TASK-10-01-04-iroh-sync-transport.md) — add iroh sync transport and qr pairing
