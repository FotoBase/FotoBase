#!/usr/bin/env python3
"""Build the expected GitHub issue body for a backlog TASK file.

Task issues are mirrors of TASK files: markdown links flattened to their text,
plus a trailing footer. Shared by tools/issues_sync_check.py (staleness check)
and the implement-task skill (status transitions: in progress / in review).

usage: python3 tools/issue_body.py <repo-relative TASK path>
Prints the expected issue body to stdout. Requires the path to be relative to
the repo root (exactly as the import footer spells it, spaces included).
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def flatten(text: str) -> str:
    """Flatten ALL [text](target) links to text — link formatting differed
    between imports, so only the *text* is meaningful for staleness."""
    return re.sub(r"\[([^\]]+)\]\(([^)]+)\)", r"\1", text)


def expected_body(task_text: str, src: str) -> str:
    """Issue body for a TASK file: flattened content + import footer."""
    return flatten(task_text).rstrip("\n") + f"\n\n---\n*Imported from `{src}`*"


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    path = Path(sys.argv[1])
    if path.is_absolute() or not (ROOT / path).exists():
        print(f"error: `{sys.argv[1]}` must be a repo-relative existing TASK path", file=sys.stderr)
        return 1
    print(expected_body((ROOT / path).read_text(encoding="utf-8"), path.as_posix()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
