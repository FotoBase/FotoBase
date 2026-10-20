# feat: print

**Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** planned

## Acceptance criteria

- Print dialog supports all listed layouts incl. contact sheet with images-per-page; shrink-to-fit vs crop-to-fit correct on paper.
- Border/text options (caption/filename/EXIF source, position, wrap) and per-photo copies render as configured.
- Printer selection works per OS; export-to-PDF produces the same layout as physical print; email via OS share sheet/`mailto` attaches auto-resized images.

- [ ] `08-004` (L) Print dialog: layouts (wallet, 3.5×5, 4×6, 5×7, 8×10, full page, contact sheet with images-per-page), shrink-to-fit vs crop-to-fit, border & text options (source caption/filename/EXIF, position, wrap), copies per photo.
- [ ] `08-005` (M) Printer selection + export-to-PDF path.
- [ ] `08-006` (S) Email via OS share sheet + `mailto` (auto-resized attachments).

## Tasks

- [ ] [TASK-08-004-print-dialog-layouts.md](TASK-08-004-print-dialog-layouts.md) — print dialog with layouts and fit options
- [ ] [TASK-08-005-printer-pdf-export.md](TASK-08-005-printer-pdf-export.md) — printer selection + PDF export
- [ ] [TASK-08-006-email-share.md](TASK-08-006-email-share.md) — email via OS share sheet / mailto
