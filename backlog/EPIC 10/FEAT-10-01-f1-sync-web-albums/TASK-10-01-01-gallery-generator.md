# task: build static html gallery generator

**Feature:** [FEAT-10-01-f1-sync-web-albums](FEAT-10-01-f1-sync-web-albums.md) · **Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (deferred)

## Description
Static HTML gallery generator for an album: resized images plus a tiny JS viewer that works offline from a file server (PLAN §7 F1 layer 1; also the project's best shareable demo artifact — ship early). Depends on EPIC-08 export targets for upload (task 02).

## Functional specs
- Selecting an album produces a self-contained static site (index.html, resized images, viewer).
- Gallery works offline from a plain file server; no backend, no build step for the viewer.
- Renders captions/titles where present.

## Technical specs
- Rust generator in the export pipeline (PLAN §6 P4 export/ modules); reuses the thumbnail/resize pipeline from EPIC-01.
- Tiny vanilla-JS viewer, no framework; deterministic output for the same album content.

## Testing criteria (TDD)
- integration: generate a gallery from a fixture album; assert expected files exist and content hashes are deterministic.
- e2e: open index.html from a static file server; viewer navigates photos.

## Acceptance criteria
- [ ] F1-001: Published album is a static site (resized images + tiny JS viewer) that works offline from a file server.
