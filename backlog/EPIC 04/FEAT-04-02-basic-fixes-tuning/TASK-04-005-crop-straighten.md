# task: implement crop and straighten ops

**Feature:** [FEAT-04-02-basic-fixes-tuning](FEAT-04-02-basic-fixes-tuning.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** in review (2026-10-30)

## Description
Crop (free + preset aspect ratios) and Straighten (small-angle rotate with auto-crop) as recipe ops on the Basic Fixes tab (PLAN §2E Basic Fixes, §6 P2b), with live preview.

## Functional specs
- Crop via draggable handles; preset aspect ratios (1:1, 4:6, etc.) plus free.
- Straighten rotates by small angles; auto-crop removes resulting empty corners.
- Both update the live preview at interactive speeds and never modify the original.

## Technical specs
- `crop` and `straighten` recipe ops in the 00-011 op model; pure functions over linear-light buffers (shared preview/export path).
- UI in the edit view's Basic Fixes tab (Svelte 5).

## Testing criteria (TDD)
- Integration: crop op on a fixture produces the golden-expected region at both preview and export resolution.
- Unit: straighten of N degrees auto-crops to the largest interior rectangle — no transparent corners.
- Golden-image test per op (per-pixel tolerance, §9).

## Acceptance criteria
- [x] 04-005: Crop (free + presets) and Straighten (small-angle, auto-crop) work as recipe ops with live preview.
- [x] 04-005: Golden-image tests pass for both ops.
