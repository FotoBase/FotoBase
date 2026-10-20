# task: set up offline pmtiles basemap

**Feature:** [FEAT-06-02-map-infrastructure](FEAT-06-02-map-infrastructure.md) · **Epic:** [EPIC-06 — Places](../EPIC-06.md) · **Status:** todo

## Description
MapLibre GL JS panel backed by a Protomaps PMTiles offline basemap, with a tile bundling strategy — fully offline, no network requests ever (PLAN §4 UI/shell "MapLibre GL JS + Protomaps PMTiles"; §6 P3 Places).

## Functional specs
- Map renders worldwide at usable zoom levels with zero network access.
- App size/first-run impact kept low per the < 20 MB installer goal — tiles bundled or downloaded deliberately with user consent.

## Technical specs
- MapLibre GL JS in the Svelte frontend (§4); PMTiles served via local file/asset protocol (no tile server).
- Tile bundle strategy (PLAN §11 Q7, decided 2026-10-20): ship a compact low-zoom world basemap (~5–10 MB) inside the installer; high-zoom regional PMTiles are opt-in downloads with explicit user consent. No silent network fetches; document the build/regenerate pipeline (protomaps build from OpenMapData sources).
- Test-enforced: no outbound network requests while the map is open.

## Testing criteria (TDD)
- E2e: open map panel offline (network blocked in test) → basemap renders and pans/zooms.
- Integration: instrument network stack during map session → zero external requests.

## Acceptance criteria
- [ ] 06-001: MapLibre + PMTiles panel renders fully offline; bundling strategy implemented.
- [ ] 06-001: No network requests ever (test-verified).
