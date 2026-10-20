# task: build ai crate with ort runtime and model downloads

**Feature:** [FEAT-05-01-ai-runtime-models](FEAT-05-01-ai-runtime-models.md) · **Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** todo

## Description
The `ai` crate: `ort` (ONNX Runtime) integration with CPU as default provider, plus the opt-in model download flow with checksums and a CI-checked license manifest — YuNet (MIT) for detection, MobileFaceNet-class (Apache-2.0) for embeddings, per the §4 pins.

## Functional specs
- Face detection + embedding run on CPU with no GPU requirement.
- Models are opt-in downloads: the app prompts before downloading; nothing downloads silently (§1 "models are opt-in downloads").
- Fully offline after download; no telemetry.

## Technical specs
- `crates/ai` (§3 module layout); `ort` with CPU execution provider default, CoreML/DirectML/CUDA optional (§4).
- Download: checksum-verified model files into the catalog location (§1 "catalog location" holds ML models).
- License manifest per model checked in CI (§10 model-licensing risk); only MIT/Apache-2.0 weights.

## Testing criteria (TDD)
- Unit: a model file with wrong checksum is rejected by the loader.
- Integration: YuNet fixture image → expected number/locations of detected faces.
- CI: license manifest fails the build if a model entry is missing or non-permissive.

## Acceptance criteria
- [ ] 05-001: `ai` crate runs YuNet + embedding on CPU via `ort` with no GPU requirement.
- [ ] 05-001: Opt-in download flow with checksum + CI-verified license manifest (MIT/Apache-2.0 only).
