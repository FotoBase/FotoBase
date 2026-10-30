# task: implement tuning tab sliders and neutral picker

**Feature:** [FEAT-04-02-basic-fixes-tuning](FEAT-04-02-basic-fixes-tuning.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** in progress (2026-10-30)

## Description
Tuning tab with Fill Light, Highlights, Shadows, and Color Temperature sliders plus a neutral-color (white balance) eyedropper picker (PLAN §2E Tuning tab), all as recipe ops with live preview.

## Functional specs
- Four sliders adjust tonal/color response; preview updates without perceptible lag.
- Neutral-color picker: click a pixel that should be neutral → white balance adjusts to it.
- Slider positions persist in the recipe and round-trip through XMP.

## Technical specs
- `fill_light`, `highlights`, `shadows`, `temperature` ops in the 00-011 model; picker computes a temperature/tint param from the sampled pixel.
- Sliders write op params; preview renders at screen res via the shared engine.

## Testing criteria (TDD)
- Integration: each slider op has a golden-image test (fixture + known param → expected pixels).
- Unit: neutral picker on a known-gray pixel yields identity/neutral white balance.

## Acceptance criteria
- [x] 04-008: Tuning sliders and neutral-color picker update the live preview without perceptible lag.
- [x] 04-008: Golden-image tests pass for each op.
