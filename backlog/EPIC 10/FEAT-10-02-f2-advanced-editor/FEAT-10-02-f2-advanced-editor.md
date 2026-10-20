# F2 — Advanced editor

**Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (after EPIC-04 recipe engine)

## Acceptance criteria

- External editor round-trip: edited result auto-imports as a stacked version; original untouched.
- Pro tab ops (curves, levels, HSL, exposure, vibrance, keystone, Lensfun, noise reduction) are recipe ops with golden-image tests — same engine, non-destructive.
- GPU pipeline produces pixel-identical output to the CPU path (or a documented tolerance).
- AI tools (masks, magic erase, AI denoise) run on-device with permissive-licensed models.

- [ ] `F2-001` (M) External editor round-trip (GIMP/Photoshop/Affinity/darktable); result auto-imports as stacked version.
- [ ] `F2-002` (L) Pro tab: curves, levels, HSL, exposure, vibrance, perspective/keystone, Lensfun lens correction, noise reduction.
- [ ] `F2-003` (XL) GPU pipeline: recipe ops ported to `wgpu` shaders, real-time full-res editing.
- [ ] `F2-004` (XL) On-device AI tools: MobileSAM/EfficientSAM masks, LaMa magic erase, AI denoise.

## Tasks

- [ ] [TASK-10-02-01-external-editor-roundtrip.md](TASK-10-02-01-external-editor-roundtrip.md) — add external editor round-trip
- [ ] [TASK-10-02-02-pro-tab-ops.md](TASK-10-02-02-pro-tab-ops.md) — build pro tab recipe ops
- [ ] [TASK-10-02-03-gpu-pipeline.md](TASK-10-02-03-gpu-pipeline.md) — port recipe ops to wgpu gpu pipeline
- [ ] [TASK-10-02-04-on-device-ai-tools.md](TASK-10-02-04-on-device-ai-tools.md) — add on-device ai editing tools
