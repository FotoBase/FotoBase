# task: prove the api with an external plugin

**Feature:** [FEAT-10-04-f5-plugin-ecosystem](FEAT-10-04-f5-plugin-ecosystem.md) · **Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (deferred)

## Description
Proof of the plugin API: at least one external plugin (metadata importer, preset pack, or gallery theme) loads and runs from outside the source tree (feature acceptance; PLAN §7 F5 — the ecosystem design only starts once ≥2 real plugins exist from EPIC-08).

## Functional specs
- A plugin developed outside the fotobase repo installs into the app and delivers its function (import, presets, or theme).

## Technical specs
- Candidate: Lightroom/Apple Photos metadata importer, or a gallery theme for F1.1/F4 — whichever the first real demand dictates.
- Uses the manifest + sandboxing from tasks 01–02 unchanged.

## Testing criteria (TDD)
- e2e: install the external plugin artifact into a clean app instance; its feature is usable.

## Acceptance criteria
- [ ] F5-003: At least one external plugin loads and runs from outside the tree as proof of the API.
