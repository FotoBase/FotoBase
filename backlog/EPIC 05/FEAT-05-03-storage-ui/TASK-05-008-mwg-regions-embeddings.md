# task: persist face regions as mwg xmp with sqlite-vec embeddings

**Feature:** [FEAT-05-03-storage-ui](FEAT-05-03-storage-ui.md) · **Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** todo

## Description
Write face regions as MWG `mwg-rs:Regions` XMP so named faces survive catalog rebuild, and store embeddings in the sqlite-vec virtual table (PLAN §5 data model; §6 P3 People).

## Functional specs
- Detected + named faces persist in the sidecar as MWG regions; other apps (digiKam) can read them.
- After catalog delete + rebuild, named faces and their regions are intact.
- Embeddings live in the sqlite-vec table for similarity search; unconfirmed faces may be recomputed (§5).

## Technical specs
- `crates/metadata` (exiv2 own bindings) writes `mwg-rs:Regions` with region rects + names; merge-on-write preserving unknown fields (§5).
- `embeddings` sqlite-vec virtual table keyed to `faces.embedding_id` (§5 schema).
- Sidecar is authoritative for user data (confirmed names); embeddings are derived cache.

## Testing criteria (TDD)
- Integration: confirm a face → sidecar contains MWG region with name; delete catalog, rebuild → face + name restored.
- Interop: sidecar round-trips through digiKam with regions intact (§9 Interop).
- Unit: embedding vector round-trips through sqlite-vec (store → query → same vector).

## Acceptance criteria
- [ ] 05-008: Face regions written as MWG `mwg-rs:Regions` XMP; embeddings in sqlite-vec.
- [ ] 05-008: Named faces survive catalog rebuild.
