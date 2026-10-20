# task: add html gallery export

**Feature:** [FEAT-08-03-misc-output](FEAT-08-03-misc-output.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
Static HTML gallery export from templates — resized images + tiny JS viewer — producing a self-contained site (PLAN §2 §J HTML export; §7 F1 layer 1: "this is also the project's best shareable demo artifact"). The same generator is reused by F1.1 web albums and F4 gift export, so build it as a standalone generator, not a UI-internal feature.

## Functional specs
- Export a selection/album/folder to a chosen destination: `index.html` + resized images + thumbnails + minimal viewer JS; zero network requests when opened (PLAN §9 F4 check).
- Templates selectable (ship ≥2); the template interface is data-driven enough that F5 themes can plug in.
- Captions/titles included per photo where present.
- Re-export is incremental-friendly: unchanged images keep their filenames so only changed files differ.

## Technical specs
- `export` crate: gallery generator — renders template files with a tiny templating step (avoid a full template-engine dependency; simple string templates), images via the EPIC-04 resize pipeline.
- Deterministic file naming (content-key or hash-based) for incremental re-export.
- F1.1 later adds "upload the generated gallery to an export plugin" — generator only produces the folder.

## Testing criteria (TDD)
- Integration: export fixture album → output contains index.html, viewer JS, all resized images; opening index.html offline (no network) renders (PLAN §9 portable-export check).
- Unit: template render with captions produces expected HTML snippets.
- Unit: re-export after one photo changes → only that photo's files differ.

## Acceptance criteria
- [ ] 08-016: HTML gallery export produces a self-contained static site reusable by F1.1/F4.
- [ ] 08-016: Gallery opens offline with zero network requests; re-export is incremental.
