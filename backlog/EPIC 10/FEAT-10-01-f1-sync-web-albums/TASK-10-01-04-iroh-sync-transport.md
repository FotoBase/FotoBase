# task: add iroh sync transport and qr pairing

**Feature:** [FEAT-10-01-f1-sync-web-albums](FEAT-10-01-f1-sync-web-albums.md) · **Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (deferred)

## Description
Transport for device sync: iroh (Rust P2P QUIC with relay fallback), device pairing via QR code, no account required (PLAN §7 F1 layer 2). Carries the CRDT document from task 03; makes EPIC-09 multi-device use convenient.

## Functional specs
- Pair a second device by scanning a QR code; no accounts or cloud signup.
- CRDT updates sync between paired devices over direct QUIC when possible, relay fallback behind NAT.
- Originals are never transferred by default (metadata document only).

## Technical specs
- iroh crate for connections + blob/sync of CRDT docs; QR payload carries the pairing ticket.
- Sync runs opportunistically in the background; battery/mobile-data friendly (batching).

## Testing criteria (TDD)
- integration: two local test peers pair via generated QR payload and exchange a CRDT update.
- integration: relay-path pairing completes when direct connection is blocked.

## Acceptance criteria
- [ ] F1-004: QR pairing completes without an account; CRDT metadata syncs over iroh with relay fallback; originals not synced by default.
