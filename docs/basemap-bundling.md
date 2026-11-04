# Bundled offline basemap

**Task:** 06-001 · **Plan refs:** §4 (MapLibre GL JS + Protomaps PMTiles), §6 P3 Places, §11 Q7 · **Decision:** [ADR-0008](./adr/adr-0008-pmtiles-bundling.md)

The map is fully offline: tiles come from a local PMTiles archive through
the `basemap://` custom scheme handler — no tile server, no network
requests, ever (test-enforced, see below).

## What ships inside the installer

`app/src-tauri/resources/basemap/world.pmtiles` — a compact low-zoom world
basemap (zoom 0–4, ~6 MB) extracted from a daily Protomaps build. It fits
the < 20 MB installer goal per ADR-0008. The frontend overzooms past z4
(vector tiles re-project; no extra requests).

## Regenerating the bundle

```
tools/basemap/build.sh <YYYYMMDD>
```

`YYYYMMDD` selects the Protomaps build day
(`https://build.protomaps.com/<day>.pmtiles`); the script extracts z0–z4
worldwide and rewrites the resource in place. Requires the
[go-pmtiles](https://github.com/protomaps/go-pmtiles) CLI. The archive is
a committed build artifact (like the app icons): regenerate it when the
basemap should be refreshed, not per build.

Source data: OpenStreetMap + Natural Earth (ODbL). The map panel renders
the required attribution; the archive metadata carries it too.

## High-zoom regional bundles (opt-in, ADR-0008)

Regional PMTiles are **opt-in downloads with explicit user consent** —
the app never fetches anything silently. When a regional archive lands
(the later Places task wires the download flow), it is stored at
`<catalog>/thumbnails/basemap/regional.pmtiles` and takes precedence over
the bundled world map for all tile requests (whole-file swap; the frontend
cache-busts via the `?v=` template parameter).

## How the tiles reach the map

1. `lib.rs` resolves the bundled archive at startup (packaged resource in
   release, crate-relative path in dev) and installs it in
   `basemap::set_paths`; without it the panel shows its empty state.
2. The `basemap://` scheme handler parses `z/x/y` addresses (both the
   direct and the WebView2 `basemap.localhost` spellings), mmaps the
   archive, looks the tile up in the PMTiles directory, gzip-decodes the
   MVT, and serves `application/vnd.mapbox-vector-tile`. Misses are 404s
   the map renders around — never error dialogs.
3. The frontend (`map-src.ts`) builds an inline MapLibre style whose only
   URL is the `basemap://` tile template, with **no `glyphs` and no
   `sprite`** — both would be network fetches. Text labels therefore do
   not render yet; they arrive with the bundled-glyph work that
   accompanies the regional-bundle flow (photo pins in 06-002 do not need
   them).

## No-network guarantees (test-enforced)

- `app/src-tauri/tests/no_network_clients.rs`: no network-client crate is
  reachable from the basemap path, and the `pmtiles` dependency is pinned
  `default-features = false` so its HTTP/S3 backends cannot even compile in.
- `app/src/lib/map-src.test.ts`: the inline style references only
  `basemap://`, declares no glyphs/sprite, and caps the source zoom at the
  bundle's range.
- `e2e/specs/map-basemap.e2e.ts`: with the map open (pan + zoom), the
  instrumented fetch/XHR stack observes zero `http(s)` requests.
