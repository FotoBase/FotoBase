# F5 — Plugin ecosystem

**Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (starts only when ≥2 real plugins exist — EPIC-08 ships several)

## Acceptance criteria

- Third-party plugins load from a manifest with sandboxing; a broken plugin can't crash or corrupt the catalog.
- At least one external plugin (importer, preset pack, or gallery theme) loads and runs from outside the tree as proof of the API.

Trait + manifest + sandboxing; metadata importers (Lightroom, Apple Photos), effect preset packs (home of the cut Picasa novelty effects), gallery themes.

## Tasks

- [ ] [TASK-10-04-01-plugin-manifest-loading.md](TASK-10-04-01-plugin-manifest-loading.md) — design plugin manifest and loading trait
- [ ] [TASK-10-04-02-plugin-sandboxing.md](TASK-10-04-02-plugin-sandboxing.md) — sandbox plugins with fault isolation
- [ ] [TASK-10-04-03-external-plugin-proof.md](TASK-10-04-03-external-plugin-proof.md) — prove the api with an external plugin
