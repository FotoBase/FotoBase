# feat: embeddings

**Epic:** [EPIC-07 — Smart search](../EPIC-07.md) · **Status:** planned

## Acceptance criteria

- CLIP/SigLIP image embeddings generate via the shared job queue with opt-in model download; permissive-only license manifest enforced in CI.
- sqlite-vec stores embeddings; text→image ANN queries return on the 50k fixture in interactive time (< ~500 ms).
- Embeddings are recomputable after catalog rebuild (§9).

- [ ] `07-001` (L) CLIP/SigLIP (OpenCLIP ViT-B/32 ONNX) image embeddings in the shared job queue; opt-in model download; license manifest in CI (permissive weights only).
- [ ] `07-002` (M) sqlite-vec storage + ANN query path for text→image search.

## Tasks

- [ ] [TASK-07-001-clip-embedding-pipeline.md](TASK-07-001-clip-embedding-pipeline.md) — CLIP image embedding pipeline
- [ ] [TASK-07-002-sqlite-vec-ann-query.md](TASK-07-002-sqlite-vec-ann-query.md) — sqlite-vec storage + ANN query path
