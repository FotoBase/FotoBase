# task: add sqlite-vec storage and ann query path

**Feature:** [FEAT-07-02-embeddings](FEAT-07-02-embeddings.md) · **Epic:** [EPIC-07 — Smart search](../EPIC-07.md) · **Status:** todo

## Description
Store the CLIP image embeddings from task 07-001 in a `sqlite-vec` virtual table (PLAN §4 Vector search, §5 `embeddings` table) and expose an ANN query path encoding a text query with the CLIP text encoder for text→image search. This is the storage/query foundation for semantic search (07-003) and auto-tagging (07-004).

## Functional specs
- A text query returns ranked photos by cosine similarity, in interactive time (< ~500 ms on the 50k fixture, per feature acceptance).
- Query results are stable across restarts; no re-scan needed after a normal shutdown.
- New/changed photos become searchable once their embedding job completes.

## Technical specs
- `catalog` crate: `sqlite-vec` extension loaded into the bundled `rusqlite` connection; `embeddings` virtual table (file_id, vec float[512]).
- Text-side: same OpenCLIP model's text encoder (ONNX, `ort`), same opt-in download and license manifest as 07-001.
- Query path: encode text → `SELECT … FROM embeddings WHERE embedding MATCH ? ORDER BY distance LIMIT k` (sqlite-vec KNN).
- Derived data: table dropped and repopulated on rebuild; face embeddings (EPIC-05/People track) share the same table pattern.

## Testing criteria (TDD)
- Unit: inserting N vectors and querying returns rows ordered by ascending cosine distance.
- Integration: given a photo of a dog on a beach and a distractor set, query "dog on the beach" ranks the dog photo in the top-k.
- Performance: 50k-fixture ANN query completes < 500 ms (record timing in the perf harness, PLAN §9).
- Integration: rebuild → embeddings table fully repopulated.

## Acceptance criteria
- [ ] 07-002: sqlite-vec virtual table stores all 50k-fixture embeddings and text→image ANN queries return in < ~500 ms.
- [ ] 07-002: Rebuild repopulates the embeddings table from recomputed vectors.
