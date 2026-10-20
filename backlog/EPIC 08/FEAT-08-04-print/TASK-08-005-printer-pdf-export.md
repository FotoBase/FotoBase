# task: add printer selection and pdf export

**Feature:** [FEAT-08-04-print](FEAT-08-04-print.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
OS printer selection inside the print dialog plus an export-to-PDF path producing the same layout as physical print (PLAN §2 §J: "printer selection or export to PDF"; §6 P4). Depends on the layout engine from 08-004.

## Functional specs
- The dialog lists installed printers per OS and prints to the chosen one with the configured layout.
- "Export to PDF" writes a PDF with identical layout/fit/text/border options to the physical path.
- Print progress/cancel feedback in the dialog.
- Printer paper size mismatch shows a warning before printing.

## Technical specs
- Tauri v2 print/plugin path per OS: system print dialogs (Windows print API, macOS NSPrintOperation/`lpr`-class, Linux CUPS); page raster from the 08-004 layout engine.
- PDF writer: a Rust PDF crate (e.g. `printpdf`/`lopdf`-class) embedding the same page rasters — one rendering path, two sinks.
- No new UI surface: choice lives in the existing dialog.

## Testing criteria (TDD)
- Integration (per OS in CI matrix, PLAN §4): print job submission to a virtual/test printer succeeds and receives the expected page count.
- Unit: PDF export of a 2-photo 4×6 layout produces a valid PDF with 1 page and expected page size.
- E2E: PDF and raster preview of the same job are layout-identical (compare cell rects).

## Acceptance criteria
- [ ] 08-005: Printer selection works per OS; export-to-PDF produces the same layout as physical print.
- [ ] 08-005: Paper-size mismatch warns before printing.
