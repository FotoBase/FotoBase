# EPIC-04 — Edit & export

**Plan ref:** §6 Phase 2b · **Size:** L · **Depends on:** EPIC-03 (sidecars, stacking)

## Acceptance

- Recipe ops produce identical results at preview and export resolution (golden images).
- RAW + JPEG pair from each major camera vendor stacks and exports correctly.

## feat: edit recipe engine

- [x] `04-001` (L) Recipe engine core: versioned ops model (00-011 spec), pure functions over linear-light buffers, same code for preview (screen-res) and export (full-res).
- [x] `04-002` (M) Recipe stored in `fb:Recipe` XMP namespace; versioning + migration path.
- [x] `04-003` (M) Undo / redo / undo all / revert; per-photo edit history.
- [x] `04-004` (M) Golden-image test harness per recipe op (per-pixel tolerance, §9).

## feat: Basic Fixes & Tuning tabs

- [x] `04-005` (M) Crop (free + preset aspect ratios) and Straighten (small-angle rotate, auto-crop).
- [x] `04-006` (M) Red-eye removal (auto + manual).
- [ ] `04-007` (M) "I'm Feeling Lucky" one-click; Auto Contrast; Auto Color.
- [ ] `04-008` (M) Tuning tab: Fill Light, Highlights, Shadows, Color Temperature sliders; neutral-color picker (white balance).
- [ ] `04-009` (M) Starter effects: Sharpen, Sepia, B&W, Saturation (strength slider).
- [ ] `04-010` (M) Edit view UI: single photo, edit tabs, filmstrip, live preview.

## feat: batch edit

- [ ] `04-011` (M) Batch ops per §2E: rotate CW/CCW, auto contrast/color, sepia, sharpen, warmify, film grain, B&W, auto red-eye, show/hide text.
- [ ] `04-012` (S) Rename with custom name + optional date/resolution suffixes.

## feat: additional decoders

- [ ] `04-013` (L) RAW decode via LibRaw bindings; embedded-JPEG-preview fallback for unsupported cameras.
- [ ] `04-014` (M) JPEG XL via jxl-oxide.

## feat: export & save

- [ ] `04-015` (L) Export dialog: original or custom resize (H/V pixel limit, slider + input), quality presets (Auto/Normal/Maximum/Minimum/Custom %), numbered filenames, text watermark; edits baked in.
- [ ] `04-016` (M) "Save" per §F spec: write edited pixels to original path, original → sibling `.originals/`, record as stacked version; Revert restores from `.originals/`.
- [ ] `04-017` (M) "Save as…" and "Save a copy" (never touch the original).
- [ ] `04-018` (M) Export engine runs recipe at full resolution (shared with 04-001); color-managed output.

## feat: duplicates

- [ ] `04-019` (M) Near-duplicate finder: perceptual hash (`image_hasher`) across catalog; review UI with keep/suggest-delete.
