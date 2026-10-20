# EPIC-07 — Smart search (CLIP, culling, memories)

**Plan ref:** §6 Phase 3, Smart search track · **Size:** XL (track, parallel with 05/06) · **Depends on:** EPIC-03

## Acceptance

- Semantic search returns relevant results for common queries on the 50k fixture.
- Auto-tags survive catalog rebuild as regular tags.

## feat: embeddings

- [ ] `07-001` (L) CLIP/SigLIP (OpenCLIP ViT-B/32 ONNX) image embeddings in the shared job queue; opt-in model download; license manifest in CI (permissive weights only).
- [ ] `07-002` (M) sqlite-vec storage + ANN query path for text→image search.

## feat: search features

- [ ] `07-003` (M) Semantic search in the live search bar ("dog on the beach at sunset"), blended with FTS results.
- [ ] `07-004` (M) Auto-tagging: content keywords from embeddings → normal tags (`dc:subject`), user-confirmable.
- [ ] `07-005` (M) Advanced search syntax: `person:`, `camera:`, `before:` field filters combinable with free text.

## feat: AI assists

- [ ] `07-006` (M) Burst grouping + AI culling: rank by sharpness/quality, pick best (feeds EPIC-03 stacking).
- [ ] `07-007` (M) Auto-rotate / horizon leveling as a recipe op (AI-estimated rotation).
- [ ] `07-008` (M) Memories: "On this day" resurfacing view.
- [ ] `07-009` (M) Rebuildability tests: embeddings recomputable, auto-tags survive as regular tags (§9).
