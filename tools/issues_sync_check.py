#!/usr/bin/env python3
"""Compare GitHub issue bodies against their source TASK files.

Expectation (matching how issues were imported): issue body = task file with
markdown links flattened to their text, plus a trailing
"*Imported from `<path>`*" footer. Reports stale/differring issues only.

Requires `gh` authenticated. Read-only (only `gh issue view`).
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

import_note_re = re.compile(r"^Imported from `(.+)`$")


def flatten(text: str) -> str:
    """Flatten ALL [text](target) links to text on both sides — link formatting
    differed between imports, so only the *text* is meaningful for staleness."""
    return re.sub(r"\[([^\]]+)\]\(([^)]+)\)", r"\1", text)


def main() -> int:
    out = subprocess.run(
        ["gh", "issue", "list", "--state", "all", "--limit", "500", "--json", "number,body"],
        capture_output=True, text=True, check=True,
    )
    issues = json.loads(out.stdout)
    stale = 0
    for issue in issues:
        lines = issue["body"].splitlines()
        src = None
        for line in reversed(lines):
            m = import_note_re.match(line.strip().strip("*"))
            if m:
                src = m.group(1)
                break
        if src is None:
            print(f"#{issue['number']}: no import footer — skipped")
            continue
        task_file = ROOT / src
        if not task_file.exists():
            print(f"#{issue['number']}: source `{src}` missing on disk")
            stale += 1
            continue
        # Reconstruct expected body: task file, links flattened, footer re-added.
        body = flatten(task_file.read_text(encoding="utf-8")).rstrip("\n")
        expected = body + f"\n\n---\n*Imported from `{src}`*"
        norm = lambda s: [re.sub(r"\s+", " ", l.strip()) for l in flatten(s).splitlines() if l.strip()]
        if norm(expected) != norm(issue["body"]):
            stale += 1
            # Show first differing line region
            a = norm(expected)
            b = norm(issue["body"])
            for i in range(max(len(a), len(b))):
                la = a[i] if i < len(a) else "<eof>"
                lb = b[i] if i < len(b) else "<eof>"
                if la != lb:
                    print(f"#{issue['number']}: differs at line {i + 1}")
                    print(f"  file  : {la}")
                    print(f"  issue : {lb}")
                    break
    print(f"\n{stale} stale / {len(issues)} issues checked")
    return 1 if stale else 0


if __name__ == "__main__":
    sys.exit(main())
