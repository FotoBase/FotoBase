# task: add on-device ai editing tools

**Feature:** [FEAT-10-02-f2-advanced-editor](FEAT-10-02-f2-advanced-editor.md) · **Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (deferred)

## Description
On-device AI tools: subject/sky masks (MobileSAM / EfficientSAM via ONNX) for local adjustments, magic erase (LaMa inpainting), AI denoise (PLAN §7 F2 item 4). All models must be permissively licensed per PLAN §1 (Apache-2.0-class weights, license manifest).

## Functional specs
- Subject/sky mask selectable for local (region-limited) recipe adjustments.
- Magic erase removes a masked region via LaMa inpainting.
- AI denoise produces a usable result on high-ISO photos.
- All inference runs on-device; no cloud calls.

## Technical specs
- ONNX runtime for MobileSAM/EfficientSAM and LaMa; masks integrate as recipe-op regions (depends on EPIC-04 engine, EPIC-05 AI runtime).
- Model weights pinned and vetted in the CI license manifest (same rule as the EPIC-05 face model).

## Testing criteria (TDD)
- integration: golden-image tests per tool on the performance fixture (mask → adjustment, erase, denoise).
- CI: license manifest check fails on a non-permissive model.

## Acceptance criteria
- [ ] F2-004: Masks, magic erase, and AI denoise run on-device with permissive-licensed models.
