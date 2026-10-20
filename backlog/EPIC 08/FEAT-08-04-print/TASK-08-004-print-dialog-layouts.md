# task: build print dialog with layouts and fit options

**Feature:** [FEAT-08-04-print](FEAT-08-04-print.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
The print dialog with Picasa's full layout set — wallet, 3.5×5, 4×6, 5×7, 8×10, full page, contact sheet with images-per-page — plus shrink-to-fit vs crop-to-fit, border & text options, and copies per photo (PLAN §2 §J Print, §6 P4). Edits are baked in via the EPIC-04 render engine.

## Functional specs
- Selecting photos and choosing Print opens the dialog with a live paper preview.
- All listed layouts available; contact sheet exposes images-per-page.
- Shrink-to-fit preserves the whole image; crop-to-fit fills the cell and crops.
- Text options: source (none/caption/filename/EXIF), font/color/size, above or below image, wrap text.
- Border options; copies per photo (e.g. 4×6 sheet with 2 copies of each).
- Preview renders through the recipe engine so printed output shows edits.

## Technical specs
- Layout engine: page templates as data (layout name → cell grid + margins); compose cells onto a page canvas in the `export`/`render` path.
- Fit modes implemented once (contain vs cover) and shared by preview and final rasterization.
- Text/border rendering reuses the text rasterization from 08-003 where practical.
- Output is a page raster handed to the OS print path (08-04-02) or PDF writer.

## Testing criteria (TDD)
- Unit: layout template math — each named layout produces expected cell rectangles on a letter page.
- Unit: contain vs cover crop of a 3:2 image in a 4×6 cell yields expected source rect.
- E2E: print a 2-photo selection with 2 copies on 4×6 → preview shows 4 cells; golden page image matches.

## Acceptance criteria
- [ ] 08-004: Print dialog supports all listed layouts incl. contact sheet with images-per-page; shrink-to-fit vs crop-to-fit correct on paper.
- [ ] 08-004: Border/text options (source, position, wrap) and per-photo copies render as configured.
