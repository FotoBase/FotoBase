# EPIC-08 — Output & sharing

**Plan ref:** §6 Phase 4 · **Size:** L · **Depends on:** EPIC-04

## Acceptance

- Each export plugin passes the contract test suite (upload, resume, retry, album mapping).
- Backup → wipe → restore reproduces library and sidecars byte-identical.

## feat: remaining editor tools

- [ ] `08-001` (M) Remaining effects: Warmify, Film Grain, Tint, Soft Focus, Vignette + frames (Border, Drop Shadow).
- [ ] `08-002` (L) Retouch heal brush (strokes as recipe op).
- [ ] `08-003` (M) Text tool: font, size, color, outline, transparency (recipe op).

## feat: print

- [ ] `08-004` (L) Print dialog: layouts (wallet, 3.5×5, 4×6, 5×7, 8×10, full page, contact sheet with images-per-page), shrink-to-fit vs crop-to-fit, border & text options (source caption/filename/EXIF, position, wrap), copies per photo.
- [ ] `08-005` (M) Printer selection + export-to-PDF path.
- [ ] `08-006` (S) Email via OS share sheet + `mailto` (auto-resized attachments).

## feat: export plugin system

- [ ] `08-007` (M) Export plugin trait + registry; shared contract test suite (upload, resume, retry, idempotency, album mapping).
- [ ] `08-008` (M) Plugin: local folder.
- [ ] `08-009` (L) Plugin: Immich (API; design for future read-back/"cloud mirror" — §1 strategic note).
- [ ] `08-010` (M) Plugin: S3-compatible.
- [ ] `08-011` (M) Plugin: WebDAV/Nextcloud.
- [ ] `08-012` (M) Plugin: Google Photos (upload-only, §10 API restriction).
- [ ] `08-013` (M) Tools → Upload dialog (batch upload with size choice).

## feat: backup

- [ ] `08-014` (L) Backup Manager: incremental backup sets of catalog + sidecars + settings to external location.
- [ ] `08-015` (M) Restore from backup sets; byte-identical restore acceptance test (§9).

## feat: misc output

- [ ] `08-016` (M) HTML gallery export (static templates; reused by F1.1/F4).
- [ ] `08-017` (M) Live/Motion photo playback (Apple Live Photos, Android Motion Photos).
- [ ] `08-018` (M) Timeline view (full-screen chronological browsing).
- [ ] `08-019` (S) Screen capture folder (PrtScn → "Screen Captures" library folder).
- [ ] `08-020` (M) OS context menu integration ("View with…", "Upload") — Explorer/Finder.
