# task: implement metadata crdt document

**Feature:** [FEAT-10-01-f1-sync-web-albums](FEAT-10-01-f1-sync-web-albums.md) · **Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (deferred)

## Description
Represent metadata (stars, tags, captions, albums, named faces, recipes) as a CRDT document (Loro or Automerge) so concurrent edits from multiple devices merge without loss (PLAN §7 F1 layer 2). Depends on the stable XMP model from EPIC-03; only confirmed face names sync — unnamed clusters are recomputed per device.

## Functional specs
- Two devices editing different fields of the same photo converge to both edits.
- Concurrent edits to the same field merge deterministically (no last-writer data loss across divergent fields).
- Only confirmed face names sync; unnamed face clusters never propagate.
- Originals are not synced — metadata only.

## Technical specs
- Loro or Automerge CRDT document per library (or per-photo); pick one, pin the version.
- Mapping layer between the XMP/sidecar model (EPIC-03) and the CRDT doc; XMP remains the on-disk truth for single-machine state.

## Testing criteria (TDD)
- unit: CRDT merge matrix — concurrent star, tag, caption, and recipe edits from two replicas converge.
- integration: confirmed face name syncs; unconfirmed cluster edits do not.

## Acceptance criteria
- [ ] F1-003: Metadata CRDT merges concurrent edits from two devices without loss; only confirmed face names sync; originals not synced by default.
