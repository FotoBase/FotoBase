---
name: implement-task
description: Implement a backlog task (TASK-NN-MMM) test-first, following its spec sections, PLAN refs, and ADRs, then sync the backlog checkboxes, set the GitHub issue status (in progress → in review), and link the opened PR to the task issue. Use when asked to implement, start, or complete a specific backlog task, task ID, or GitHub task URL.
---

# Implement a backlog task

## 0. Sync main

Before anything else, get the latest main — the TASK file, issue body, and predecessor checkboxes must be read from current state, not a stale checkout:

```
git switch main
git pull --ff-only origin main
```

If `--ff-only` fails (local main has diverged), stop and report instead of merging on your own.

## 1. Open the task

- From a GitHub task URL or issue number (`https://github.com/FotoBase/FotoBase/issues/<n>`): run `gh issue view <n> --json body` — never a bare `gh issue view`, it opens a pager and wedges the terminal. Parse the trailing `*Imported from \`backlog/...\`*` footer; that repo-relative path is the TASK file. No footer → stop and report (not an imported task).
- From a task ID (`NN-MMM`): locate `backlog/EPIC NN/FEAT-NN-MM-name/TASK-NN-MMM-slug.md` directly. Find the mirrored issue with `gh issue list --search "<NN-MMM>" --json number,title` (JSON output only, same pager rule); if there is no matching issue, skip the issue syncs in steps 2 and 6 and say so.
- Read the TASK file completely. Note the `Feature`/`Epic` links and every PLAN §ref and ADR citation; read those sections before writing code.

## 2. Set the GitHub issue status to `in progress`

Starting work on a task means its GitHub issue shows `in progress` before any code is written. The TASK file is the source of truth; the issue body is its mirror (links flattened + import footer). Never hand-edit an issue body — regenerate it:

```
python3 tools/issue_body.py "<repo-relative task path>" > /tmp/issue-body.md
gh issue edit <n> --body-file /tmp/issue-body.md
```

- Edit the task's `**Status:**` line to `in progress (<today's date>)`, then run the two commands above. This is what actually publishes the status to GitHub — editing the TASK file alone does nothing to the issue.
- Completion criterion: `python3 tools/issues_sync_check.py` reports `0 stale` (this also confirms the regenerated body is byte-exact), and the issue body's Status line reads `in progress`.

## 3. Pre-flight

- Read `.github/copilot-instructions.md` — architecture invariants and stack pins are non-negotiable.
- Check the epic (`backlog/EPIC NN/EPIC-0N.md`) and FEAT files: if predecessor tasks this task depends on are unchecked, stop and report instead of implementing out of order.
- If the task would contradict an accepted ADR, stop: per CONTRIBUTING.md and ADR-0001, the ADR must first be superseded by a new one.
- Pre-flight passed: create the task branch **from the freshly pulled main before any further edits** (uncommitted step-2 status changes carry over cleanly): `git switch -c feat/<task-id>-slug` or `fix/<task-id>-slug`.

## 4. Work test-first

- Write the tests described in the task's **Testing criteria (TDD)** section FIRST and watch them fail.
- Use the right layer's tooling: `cargo test` + `insta`/golden-image for Rust, Vitest + Testing Library for the frontend, WebdriverIO + `tauri-driver` for E2E.
- Then implement per the **Functional specs** and **Technical specs** sections, respecting the invariants (originals read-only, sidecars source of truth, no image bytes over IPC, job queue for heavy work, one recipe engine).
- If the task touches metadata writes, follow `docs/sidecar-merge-on-write.md` exactly — this is the highest data-loss-risk area.

## 5. Verify

- Run the task's tests plus the layer's lint/format checks (`docs/workflows/lint-format.md`).
- Check every bullet in the task's **Acceptance criteria** section against what was built; report each as met/not met with evidence.
- If the task has perf implications, note that the `perf-acceptance` skill applies.

## 6. Close the loop: open the PR, then set `in review`

- Check off the task's checkbox in BOTH the epic file and the FEAT file (they are mirrored 1:1), and the task's own acceptance-criteria boxes.
- Open the PR from the branch created in step 3 (`feat/<task-id>-slug`) FIRST, so the status flip lands on an issue that already shows the PR:
  - Conventional commit title with the task ID, link the task file path, `Task: NN-MMM` footer in the commit message.
  - The PR body MUST contain `Closes #<n>` — that closing keyword is what makes GitHub link the PR to the task issue (Development section + issue timeline). A bare `Refs #<n>` does NOT create that link. Merging the PR then closes the issue and sets `done` in one step; don't try to link the PR after the fact with issue edits.
- Set the `**Status:**` line to `in review (<today's date>)` and resync the issue with the two step-2 commands. `done` is set at PR merge, not here.
- If you had to make a spec-level decision not covered by PLAN or an ADR, say so in the PR and suggest whether it warrants an ADR.
