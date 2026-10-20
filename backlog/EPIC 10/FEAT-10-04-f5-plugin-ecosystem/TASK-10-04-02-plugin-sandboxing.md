# task: sandbox plugins with fault isolation

**Feature:** [FEAT-10-04-f5-plugin-ecosystem](FEAT-10-04-f5-plugin-ecosystem.md) · **Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (deferred)

## Description
Sandbox plugins so a broken plugin can't crash the app or corrupt the catalog (feature acceptance). Coarse by design; refined when the ecosystem starts (≥2 real plugins, PLAN §7 F5).

## Functional specs
- A plugin that panics or hangs is disabled with an error message; the app keeps running.
- Plugins cannot write outside their granted scope; catalog writes go through a vetted API, never raw file access.
- Plugin failure never leaves the catalog in a corrupted state.

## Technical specs
- Process-isolated or WASM-style sandbox to be chosen at refinement; panic/hang timeouts enforced by the host regardless of mechanism.
- Capability grants declared in the manifest (task 01), enforced by the host.

## Testing criteria (TDD)
- integration: fixture plugin that panics, one that hangs, one that attempts an out-of-scope write — all are contained, app and catalog intact.

## Acceptance criteria
- [ ] F5-002: A broken plugin cannot crash or corrupt the catalog; out-of-scope access is denied.
