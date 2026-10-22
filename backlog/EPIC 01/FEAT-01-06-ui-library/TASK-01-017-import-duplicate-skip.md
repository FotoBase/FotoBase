# task: build import with destination naming and duplicate skip

**Feature:** [FEAT-01-06-ui-library](FEAT-01-06-ui-library.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in review (2026-10-22)

## Description
Implement Import from folder / removable drive per PLAN §2A/§6 P1: destination folder naming (date / custom) and duplicate skipping with an "Exclude duplicates" checkbox, default on (per §2 Options: "Detect duplicates while importing", default-enabled).

## Functional specs
- Pick a source (folder / removable drive), pick destination naming (date-based or custom pattern), import copies files into the catalog location.
- "Exclude duplicates" checkbox (default on): files whose content hash already exists in the catalog are skipped and reported.
- Import runs through the job queue with progress; cancel leaves no half-copicked batch (per-file cleanup).

## Technical specs
- Duplicate detection by content hash against the `files` table (exact duplicates; perceptual-hash near-dupes are P2 §2B).
- Destination naming: date folders from EXIF taken_at when available, file date otherwise; custom pattern per user input.

## Testing criteria (TDD)
- Integration: import a folder containing a file already in the catalog with dup-exclusion on ⇒ file skipped, counted in the report; off ⇒ copied.
- Integration: cancel mid-import ⇒ no partial files at the destination.

## Acceptance criteria
- [x] 01-017: Import from folder/removable drive with date/custom destination naming and duplicate skip (checkbox default on).
