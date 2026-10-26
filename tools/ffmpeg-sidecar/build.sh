#!/usr/bin/env bash
# TASK-02-003: build the LGPL ffmpeg sidecar binary (PLAN §2 Video strategy,
# §10 risk row: "ship LGPL build without non-free codecs").
#
# Builds ffmpeg from the pinned upstream source tarball with an explicitly
# LGPL-only configuration — no --enable-gpl, no --enable-nonfree, no external
# GPL encoders (libx264/libx265…). check.sh re-verifies the *resulting
# binary*, so a configuration drift fails CI instead of shipping.
#
# Usage: fetch.sh <output-dir>   (ffmpeg binary lands in <output-dir>/ffmpeg)
set -euo pipefail

FFMPEG_VERSION="7.1.1"
FFMPEG_SHA256="733984395e0dbbe5c046abda2dc49a5544e7e0e1e2366bba849222ae9e3a03b1"
URL="https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz"

out_dir="${1:?usage: fetch.sh <output-dir>}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo ">> downloading ffmpeg ${FFMPEG_VERSION}"
curl -sSL -o "$work/ffmpeg.tar.xz" "$URL"
echo "$FFMPEG_SHA256  $work/ffmpeg.tar.xz" | shasum -a 256 -c -

tar -xf "$work/ffmpeg.tar.xz" -C "$work"
cd "$work/ffmpeg-${FFMPEG_VERSION}"

echo ">> configuring (LGPL-only: no gpl, no nonfree, no external GPL libs)"
./configure \
    --disable-gpl \
    --disable-nonfree \
    --disable-doc \
    --disable-debug \
    --disable-network \
    --disable-autodetect \
    --enable-protocol=file \
    --enable-protocol=pipe \
    --enable-protocol=fd \
    --disable-outdevs \
    --prefix="$work/install"

echo ">> building"
make -j"$(getconf _NPROCESSORS_ONLN 2>/dev/null || nproc 2>/dev/null || echo 2)" ffmpeg

mkdir -p "$out_dir"
install -m 0755 ./ffmpeg "$out_dir/ffmpeg"
echo ">> sidecar ffmpeg at $out_dir/ffmpeg"
