# feat: AI runtime & models

**Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** planned

## Acceptance criteria

- `ai` crate runs YuNet detection + embedding on CPU via `ort` with no GPU requirement; opt-in download flow with checksum + license manifest verified in CI.
- Detection/embedding run as job-queue jobs honoring battery throttling; UI stays responsive during a full-library face pass.
- Models are permissively licensed (MIT/Apache-2.0) — usable in GPL distribution without exceptions.

- [ ] `05-001` (L) `ai` crate: `ort` (ONNX Runtime) integration, CPU default; opt-in model download flow (license manifest checked in CI — YuNet detect MIT, MobileFaceNet-class embed Apache-2.0, per §4 pins).
- [ ] `05-002` (M) Face detection (YuNet) + embedding as job-queue jobs; battery throttling hook.

## Tasks

- [ ] [TASK-05-001-ort-runtime-models.md](TASK-05-001-ort-runtime-models.md) — build ai crate with ort runtime and model downloads
- [ ] [TASK-05-002-face-jobs-throttle.md](TASK-05-002-face-jobs-throttle.md) — run face detection and embedding as throttled jobs
