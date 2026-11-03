#!/usr/bin/env bash
# Download the AI models listed in crates/ai/model-manifest.json into
# target/ai-models, verifying SHA-256 and size (TASK-05-001).
#
# Usage: tools/download_models.sh [models-dir]
# No network access happens without running this script explicitly —
# models are opt-in downloads (PLAN §1).
set -euo pipefail

MANIFEST="$(dirname "$0")/../crates/ai/model-manifest.json"
DEST="${1:-$(dirname "$0")/../target/ai-models}"

mkdir -p "$DEST"
python3 - "$MANIFEST" "$DEST" <<'EOF'
import hashlib, json, sys, urllib.request
from pathlib import Path

manifest, dest = Path(sys.argv[1]), Path(sys.argv[2])
for entry in json.loads(manifest.read_text()):
    name = entry["name"]
    out = dest / name
    if out.is_file() and out.stat().st_size == entry["size_bytes"]:
        h = hashlib.sha256(out.read_bytes()).hexdigest()
        if h == entry["sha256"]:
            print(f"ok (cached): {name}")
            continue
    print(f"downloading {name} ...")
    tmp = out.with_suffix(out.suffix + ".tmp")
    with urllib.request.urlopen(entry["url"]) as r, open(tmp, "wb") as f:
        while chunk := r.read(1 << 20):
            f.write(chunk)
    data = tmp.read_bytes()
    if len(data) != entry["size_bytes"]:
        sys.exit(f"FAIL size {name}: {len(data)} != {entry['size_bytes']}")
    if hashlib.sha256(data).hexdigest() != entry["sha256"]:
        tmp.unlink()
        sys.exit(f"FAIL checksum {name}")
    tmp.rename(out)
    print(f"ok: {name}")
EOF
