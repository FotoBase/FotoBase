# task: multiple catalogs and catalog switcher

**Feature:** [FEAT-03-04-picasa-migration-portability](FEAT-03-04-picasa-migration-portability.md) · **Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** in review (2026-10-30)

## Description
Optional multiple catalogs (e.g. work/personal) with a switcher UI, per PLAN §6 P2a; the catalog location is user-configurable (§1 "Catalog" decision), so switching must isolate caches, watchers, and ML state completely.

## Functional specs
- Create additional catalogs from options; switch via a catalog switcher; switching requires no app restart beyond a reload.
- Each catalog keeps its own watched folders, thumbnail cache, ML models, and watcher state — no cross-contamination.
- Last-used catalog reopens on launch.

## Technical specs
- Catalog registry (small JSON list of catalog locations) separate from any catalog; per-catalog directory already holds DB + cache + models (§1).
- On switch: stop watchers, flush job queue, close DB (WAL checkpoint), reopen target catalog, restart watchers — reuse the tray-icon lifecycle (03-025).
- Switcher UI in Options + tray menu.

## Testing criteria (TDD)
- Integration: two fixture catalogs — star a photo in A, switch to B, assert B's FTS/watcher/scan state shows nothing from A and back in A the star persists.
- Unit: registry create/switch/last-used logic.

## Acceptance criteria
- [x] 03-024: user can create and switch between multiple catalogs.
- [x] 03-024: no cross-contamination of caches, watcher state, or catalog data.
- [x] 03-024: last-used catalog opens on launch.
