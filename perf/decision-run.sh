#!/usr/bin/env bash
# TASK-01-004 decision run (docs/webkitgtk-perf-protocol.md §4) inside the
# perf/Dockerfile.decision container. Runs the FULL 50k protocol twice — the
# two runs on the same seed are the task's reproducibility criterion (§TDD).
#
# Host usage (Linux x64 machine, repo checked out at $PWD):
#   docker build -f perf/Dockerfile.decision -t fotobase-perf .
#   docker run --rm -it --shm-size=2g --ipc=host \
#       -v "$PWD":/repo -w /repo fotobase-perf bash perf/decision-run.sh
#
# Reports land in perf/results/ on the host (bind mount). Exit code mirrors
# the harness: 0 all §2 bars pass, 1 a bar missed, 2 harness error.
set -euo pipefail

count="${1:-50000}"
cd /repo

# Container quirks: force X11 + disable the DMABUF renderer (no GPU in
# containers); give WebKit a writable XDG runtime dir.
export DEBIAN_FRONTEND=noninteractive
export GDK_BACKEND=x11
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export XDG_RUNTIME_DIR=/tmp/xdg-runtime
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"

echo "== runner info =="
uname -a
nproc
free -g | head -2
pkg-config --modversion webkit2gtk-4.1
df -h /tmp | tail -1

echo "== fixture (00-013, seed 42, count $count) =="
python3 tools/fixture_gen.py --count "$count" --seed 42 --out /tmp/fixture

echo "== build app =="
cargo build -p fotobase

echo "== harness unit tests =="
cd perf
npm ci
npx vitest run

echo "== protocol run 1 of 2 =="
ts="$(date +%Y%m%d-%H%M%S)"
xvfb-run -a npx tsx src/harness.ts \
    --fixture /tmp/fixture --count "$count" --seed 42 \
    --renderer dom --out "results/run1-$ts.json"

echo "== protocol run 2 of 2 (reproducibility) =="
xvfb-run -a npx tsx src/harness.ts \
    --fixture /tmp/fixture --count "$count" --seed 42 \
    --renderer dom --out "results/run2-$ts.json"

echo "== done =="
ls -l results/
echo "Reports: perf/results/run1-$ts.json perf/results/run2-$ts.json"
