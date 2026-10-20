#!/usr/bin/env python3
"""Docs lint for the fotobase backlog and ADRs.

Checks (per .github/skills/backlog-maintenance):
  1. Epic vs FEAT checkbox lines mirror 1:1 (task ID, size, wording).
  2. ADR files are sequential adr-NNNN-* and use the standard sections.
  3. Relative markdown links resolve to existing files.

Exit 0 = clean, 1 = problems found. Deterministic, stdlib only.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# Task IDs: `NN-MMM` (epics 00-09) or `FN-MMM` (EPIC-10 futures, e.g. F1-001).
TASK_RE = re.compile(r"^\s*- \[ \] (`(?:\d\d|F\d)-\d\d\d`) \((S|M|L|XL)\) (.*)$")
LINK_RE = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
ADR_NAME_RE = re.compile(r"^adr-(\d{4})-.+\.md$")

problems: list[str] = []


def err(msg: str) -> None:
    problems.append(msg)


def checkbox_lines(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    for i, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        m = TASK_RE.match(line)
        if m:
            tid = m.group(1).strip("`")
            if tid in out:
                err(f"{path}:{i}: duplicate task checkbox `{tid}`")
            out[tid] = line.strip()
    return out


def check_backlog_mirror() -> None:
    backlog = ROOT / "backlog"
    for epic_dir in sorted(backlog.glob("EPIC ??")):
        epic_file = next(epic_dir.glob("EPIC-*.md"), None)
        if epic_file is None:
            err(f"{epic_dir}: no EPIC-*.md file")
            continue
        epic_tasks = checkbox_lines(epic_file)
        feat_tasks: dict[str, str] = {}
        for feat_dir in sorted(epic_dir.glob("FEAT-*")):
            for feat_file in feat_dir.glob("FEAT-*.md"):
                for tid, line in checkbox_lines(feat_file).items():
                    if tid in feat_tasks:
                        err(f"{feat_file}: task `{tid}` already listed by another FEAT file")
                    feat_tasks[tid] = line
        for tid in sorted(set(epic_tasks) | set(feat_tasks)):
            if tid not in epic_tasks:
                err(f"{epic_file.name}: task `{tid}` in FEAT files but missing from epic")
            elif tid not in feat_tasks:
                err(f"{epic_file.name}: task `{tid}` in epic but missing from FEAT files")
            elif epic_tasks[tid] != feat_tasks[tid]:
                err(
                    f"{epic_file.name}: task `{tid}` checkbox line differs from FEAT file:\n"
                    f"  epic: {epic_tasks[tid]}\n  feat: {feat_tasks[tid]}"
                )


def check_adrs() -> None:
    adr_dir = ROOT / "docs" / "adr"
    files = sorted(adr_dir.glob("adr-*.md"))
    expected = 1
    for f in files:
        m = ADR_NAME_RE.match(f.name)
        if not m:
            err(f"{f}: ADR filename does not match adr-NNNN-slug.md")
            continue
        n = int(m.group(1))
        if n != expected:
            err(f"{f}: expected adr-{expected:04d} (sequence gap/duplicate)")
            expected = n
        expected += 1
        text = f.read_text(encoding="utf-8")
        for marker in ("**Date:**", "**Status:**", "## Context", "## Decision", "## Consequences"):
            if marker not in text:
                err(f"{f}: missing standard ADR section {marker}")


def check_links() -> None:
    for md in ROOT.rglob("*.md"):
        if ".venv" in md.parts or "node_modules" in md.parts:
            continue
        base = md.parent
        for i, line in enumerate(md.read_text(encoding="utf-8").splitlines(), 1):
            for target in LINK_RE.findall(line):
                if target.startswith(("http://", "https://", "#", "mailto:")):
                    continue
                path = target.split("#")[0]
                if not path:
                    continue
                if not (base / path).exists():
                    err(f"{md}:{i}: broken link `{target}`")


def main() -> int:
    check_backlog_mirror()
    check_adrs()
    check_links()
    if problems:
        print(f"docs-lint: {len(problems)} problem(s)")
        for p in problems:
            print(f"  - {p}")
        return 1
    print("docs-lint: clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())
