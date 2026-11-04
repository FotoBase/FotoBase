#!/bin/sh
# Regenerate the bundled offline basemap (TASK-06-001, ADR-0008).
#
# Ships a compact low-zoom world basemap (~6 MB, z0–z4) inside the
# installer; high-zoom regional PMTiles are opt-in downloads with explicit
# user consent (ADR-0008) — this script bundles only the world basemap.
#
# Source: the daily Protomaps build (vector tiles derived from
# OpenStreetMap + Natural Earth, ODbL — attribution is rendered in the map
# panel and embedded in the archive metadata).
#
# Usage: tools/basemap/build.sh <YYYYMMDD> [out]
#   YYYYMMDD  the Protomaps build day (https://build.protomaps.com/<day>.pmtiles)
#   out       output path; default app/src-tauri/resources/basemap/world.pmtiles
#
# Requires the go-pmtiles CLI on PATH
# (https://github.com/protomaps/go-pmtiles releases).
set -eu

day="$1"
out="${2:-$(dirname "$0")/../../app/src-tauri/resources/basemap/world.pmtiles}"

mkdir -p "$(dirname "$out")"
pmtiles extract "https://build.protomaps.com/${day}.pmtiles" "$out" --maxzoom=4
echo "wrote $out:"
ls -la "$out"
pmtiles show "$out" | head -8
