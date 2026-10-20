---
name: backlog-maintenance
description: Keep the backlog and docs in sync after merged PRs, and run a periodic doc-lint sweep (checkbox sync, ADR index, PLAN refs, task numbering). Use after merging task PRs, when checkboxes look stale, or when asked to audit the backlog.
---

# Backlog maintenance

## After each merged task PR

1. Confirm the task's checkbox is checked in BOTH places (they mirror 1:1):
   - the epic file `backlog/EPIC NN/EPIC-0N.md`
   - the FEAT file `backlog/EPIC NN/FEAT-NN-MM-name/FEAT-*.md` (both the inline `- [ ] \`NN-MMM\``line and the`## Tasks` list entry)
2. Update the TASK file's `**Status:**` line (todo → done).
3. Confirm the branch/PR task ID maps to a real TASK file — no orphan references.

## Periodic doc-lint sweep

- **ADR index:** every `docs/adr/adr-NNNN-*.md` is sequential, has the standard sections (Date · Status · Context · Decision · Consequences), and is referenced from wherever it's cited. New ADRs get the next free number (currently after adr-0012).
- **PLAN refs:** sample TASK files and verify their `§refs` resolve in `docs/PLAN.md`.
- **Numbering audit:** task IDs `NN-MMM` are unique, epic and FEAT checkbox lines match the TASK files present, and no duplicate/gap anomalies exist. (A previous audit lives in `tools/audit_numbering.ipynb` — reuse its checks; beware that findall vs match group indexing differs, see that notebook's history.)
- **Checklist mirroring:** diff the checkbox lines between each epic and its FEAT files; they must be identical in ID, size marker, and wording.
- **Stale names:** PLAN §3 still shows a pre-ADR-0004 workspace dir name — the repo/project is "Fotobase" (ADR-0004). Report stale references; don't rewrite PLAN wholesale.

## Reporting

Report findings as a list: file, line, problem, suggested fix. Checkbox-sync fixes can be applied directly; structural renumbering proposals go to the user first.
