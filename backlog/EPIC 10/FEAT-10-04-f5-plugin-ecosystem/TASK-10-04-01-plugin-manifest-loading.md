# task: design plugin manifest and loading trait

**Feature:** [FEAT-10-04-f5-plugin-ecosystem](FEAT-10-04-f5-plugin-ecosystem.md) · **Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (deferred)

## Description
Define the third-party plugin format: a manifest describing the plugin, and a loading path built on the EPIC-08 export plugin trait (PLAN §7 F5: trait + manifest + sandboxing; starts only once ≥2 real plugins exist from EPIC-08). Deferred until then.

## Functional specs
- A plugin ships as a folder/archive with a manifest (id, version, type, entry point).
- The app lists installed plugins and loads them at startup; unknown manifest fields are tolerated.
- Plugin types cover metadata importers, effect preset packs, and gallery themes (PLAN §7 F5 list).

## Technical specs
- Extend the EPIC-08 export plugin trait into the general plugin trait; no new runtime/inventory system.
- Manifest is a versioned declarative file (TOML/JSON); loading is explicit and logged.

## Testing criteria (TDD)
- integration: a fixture plugin with a valid manifest loads; a malformed manifest fails with a clear error and is skipped.

## Acceptance criteria
- [ ] F5-001: Plugins load from a manifest; supported types include importer, preset pack, and gallery theme.
