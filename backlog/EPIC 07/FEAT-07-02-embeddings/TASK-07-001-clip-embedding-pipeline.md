# task: add clip image embedding pipeline

**Feature:** [FEAT-07-02-embeddings](FEAT-07-02-embeddings.md) · **Epic:** [EPIC-07 — Smart search](../EPIC-07.md) · **Status:** todo

## Description
Compute OpenCLIP ViT-B/32 (ONNX) image embeddings for every cataloged photo via the shared job queue (PLAN §6 Phase 3 Smart search). Model is an opt-in download (PLAN §1 "Private AI"); the CI license manifest must reject non-permissive weights (PLAN §10 model-licensing risk). Depends on the AI runtime from EPIC-05 and the job queue from EPIC-01.

## Functional specs
- On first use of any smart-search feature, the user is prompted to opt in to the model download (~ViT-B/32 size); no download happens without consent.
- Embeddings are computed in the background at low priority; visible thumbnails are never starved.
- Job queue throttles when on battery (PLAN §3 rule 4).
- Embeddings are recomputable derived data: deleting the catalog and rebuilding re-derives them (PLAN §5 authority split — never written to sidecars).

## Technical specs
- `ai` crate: `ort` (ONNX Runtime) session, CPU by default; CoreML/DirectML providers optional (PLAN §4).
- Input: libvips-decoded image resized to 224×224, standard CLIP normalization; output 512-d float vector, L2-normalized.
- Jobs run through the same priority job queue as face detection (PLAN §6 P3: "computed in the same job queue as face detection").
- Model files stored under the catalog location; SHA-256 pinned; CI license manifest (permissive only — CC-BY/Apache weights) gates release builds (PLAN §4 Semantic search row, §10).

## Testing criteria (TDD)
- Unit: given a fixture JPEG, the embedding job produces a 512-d vector with L2 norm ≈ 1.0.
- Integration: enqueue 100 photos → queue drains without exceeding the configured memory cap; priority honored (visible-item jobs run before background).
- CI: license manifest test fails if a model with a non-permissive license is added.
- Integration: catalog rebuild → embeddings recomputed for all photos (feeds 07-009).

## Acceptance criteria
- [ ] 07-001: Opt-in download flow downloads OpenCLIP ViT-B/32 ONNX weights and computes embeddings for all photos in the 50k fixture via the job queue, throttled on battery.
- [ ] 07-001: CI license manifest fails the build on non-permissive model weights.
- [ ] 07-001: No embedding is ever written to an XMP sidecar (derived data stays in the catalog).
