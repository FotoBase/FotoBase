# task: build pdf photo book export

**Feature:** [FEAT-10-03-f4-gift-export](FEAT-10-03-f4-gift-export.md) · **Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (deferred)

## Description
PDF photo book with auto grid layouts, print-shop-ready page sizes and bleed; rendering is deterministic from a given album (PLAN §7 F4). Depends on EPIC-04 export pipeline.

## Functional specs
- Choose an album → get a PDF book with automatically laid out photo grids.
- Standard print-shop page sizes with correct bleed margins.
- Deterministic: same album + settings produce the same PDF.

## Technical specs
- Rust PDF generation in the export pipeline; layout engine uses simple grid packing (no layout-solver dependency).
- Page size/bleed as export options; deterministic ordering by album order.

## Testing criteria (TDD)
- integration: export a fixture album; assert page count, page size, and a stable content hash across two runs.

## Acceptance criteria
- [ ] F4-002: PDF photo book has auto grid layouts and print-shop-ready page sizes/bleed; deterministic rendering from a given album.
