#!/usr/bin/env python3
"""CI gate for the AI model license manifest (TASK-05-001, PLAN §4 pins).

Every entry in `crates/ai/model-manifest.json` must carry a permissive
license (MIT or Apache-2.0 only) and a pinned download URL. PLAN §1/§10:
no non-commercial weights (InsightFace et al.) may enter the app, and the
manifest is the enforcement point checked in CI.

Exit code 0 = manifest passes; 1 = missing entry / non-permissive license /
missing field.
"""

import json
import sys
from pathlib import Path

ALLOWED_LICENSES = {"MIT", "Apache-2.0"}
REQUIRED_FIELDS = ("name", "license", "url", "sha256", "size_bytes")

MANIFEST = Path(__file__).resolve().parent.parent / "crates" / "ai" / "model-manifest.json"


def main() -> int:
    if not MANIFEST.is_file():
        print(f"FAIL: manifest not found: {MANIFEST}", file=sys.stderr)
        return 1
    try:
        entries = json.loads(MANIFEST.read_text())
    except json.JSONDecodeError as e:
        print(f"FAIL: manifest is not valid JSON: {e}", file=sys.stderr)
        return 1
    if not isinstance(entries, list) or not entries:
        print("FAIL: manifest must be a non-empty JSON array", file=sys.stderr)
        return 1

    failures = []
    seen = set()
    for i, entry in enumerate(entries):
        label = entry.get("name", f"<entry {i}>")
        for field in REQUIRED_FIELDS:
            if field not in entry or entry[field] in (None, ""):
                failures.append(f"{label}: missing required field '{field}'")
        if label in seen:
            failures.append(f"{label}: duplicate entry")
        seen.add(label)
        lic = entry.get("license")
        if lic is not None and lic not in ALLOWED_LICENSES:
            failures.append(
                f"{label}: license '{lic}' is not permissive "
                f"(allowed: {', '.join(sorted(ALLOWED_LICENSES))})"
            )
        url = entry.get("url")
        if url is not None and not str(url).startswith("https://"):
            failures.append(f"{label}: url must be https")

    if failures:
        for f in failures:
            print(f"FAIL: {f}", file=sys.stderr)
        return 1
    print(f"OK: {len(entries)} model entries, all permissive (MIT/Apache-2.0)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
