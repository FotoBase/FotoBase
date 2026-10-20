# task: build full-resolution color-managed export engine

**Feature:** [FEAT-04-06-export-save](FEAT-04-06-export-save.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** todo

## Description
The export engine that runs the recipe at full resolution — the same engine shared with the preview path from 04-001 — with color-managed output (PLAN §3 rule 5; §6 P2b; §9 Render fidelity).

## Functional specs
- Exports and Saves render the recipe at full resolution with results identical to the preview (golden images).
- Output colors are color-managed: source profile honored, output tagged; matches the color-managed preview from EPIC-02.

## Technical specs
- One engine, two resolutions (preview screen-res, export full-res) — no separate export op implementations.
- Color management: honor embedded ICC profiles and the display profile chain from FEAT-02-01; tag exported files with the appropriate profile.
- Runs in the job queue; memory-bounded streaming for very large images.

## Testing criteria (TDD)
- Integration: golden-image test — recipe rendered at preview res vs full res, outputs match within per-pixel tolerance (§9).
- Integration: image with embedded ICC profile exports tagged and visually consistent with preview.
- Unit: very-large-image render stays within memory budget (streamed tiles).

## Acceptance criteria
- [ ] 04-018: Export engine runs the recipe at full resolution, shared with the preview code path.
- [ ] 04-018: Exported output is color-managed and matches golden images.
