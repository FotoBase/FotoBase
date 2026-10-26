#!/usr/bin/env bash
# TASK-02-003: verify a candidate ffmpeg sidecar is an LGPL build with no
# non-free codecs. Runs the binary itself (fotobase-video::license_check
# encodes the same rules in Rust for in-app use); CI runs this on all three
# OSes against the pinned build from build.sh.
#
# Usage: check.sh <path-to-ffmpeg>
set -euo pipefail

ffmpeg="${1:?usage: check.sh <path-to-ffmpeg>}"

fail() { echo "FAIL: $1" >&2; exit 1; }

version="$("$ffmpeg" -version | head -1)" || fail "cannot run $ffmpeg"
echo ">> $version"

config="$("$ffmpeg" -hide_banner -version | grep '^configuration:' || true)"
case "$config" in
    *--enable-gpl*)     fail "configuration contains --enable-gpl" ;;
    *--enable-nonfree*) fail "configuration contains --enable-nonfree" ;;
esac
echo ">> configuration is LGPL-clean"

# Non-free / GPL encoders must not exist in the shipped build (libx264 and
# libx265 are GPL; nvenc/qsv/amf/videotoolbox builds are nonfree).
encoders="$("$ffmpeg" -hide_banner -encoders 2>/dev/null || "$ffmpeg" -encoders 2>/dev/null)"
for enc in libx264 libx265 libxvid libaom-av1 libsvtav1 h264_nvenc hevc_nvenc \
           h264_amf hevc_amf h264_qsv hevc_qsv h264_videotoolbox hevc_videotoolbox; do
    if echo "$encoders" | grep -q " $enc "; then
        fail "forbidden encoder present: $enc"
    fi
done
echo ">> no non-free encoders"

# Baseline encoders the transcode fallback (TASK-02-006) relies on must exist.
for enc in mpeg4 libvpx-vp9; do
    echo "$encoders" | grep -q " $enc " || fail "missing required encoder: $enc"
done
echo ">> required LGPL encoders present"
echo "OK: $version is an LGPL build with no non-free codecs"
