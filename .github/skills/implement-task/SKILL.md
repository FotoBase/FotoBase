---
name: implement-task
description: Implement a backlog task (TASK-NN-MMM) test-first, following its spec sections, PLAN refs, and ADRs, then sync the backlog checkboxes. Use when asked to implement, start, or complete a specific backlog task or task ID.
---

# Implement a backlog task

## 1. Locate and read the task

- Find `backlog/EPIC NN/FEAT-NN-MM-name/TASK-NN-MMM-slug.md` for the given task ID (or ask the user which task).
- Read it completely. Note the `Feature`/`Epic` links and every PLAN §ref and ADR citation; read those sections before writing code.

## 2. Pre-flight

- Read `.github/copilot-instructions.md` — architecture invariants and stack pins are non-negotiable.
- Check the epic (`backlog/EPIC NN/EPIC-0N.md`) and FEAT files: if predecessor tasks this task depends on are unchecked, stop and report instead of implementing out of order.
- If the task would contradict an accepted ADR, stop: per CONTRIBUTING.md and ADR-0001, the ADR must first be superseded by a new one.

## 3. Work test-first

- Write the tests described in the task's **Testing criteria (TDD)** section FIRST and watch them fail.
- Use the right layer's tooling: `cargo test` + `insta`/golden-image for Rust, Vitest + Testing Library for the frontend, WebdriverIO + `tauri-driver` for E2E.
- Then implement per the **Functional specs** and **Technical specs** sections, respecting the invariants (originals read-only, sidecars source of truth, no image bytes over IPC, job queue for heavy work, one recipe engine).
- If the task touches metadata writes, follow `docs/sidecar-merge-on-write.md` exactly — this is the highest data-loss-risk area.

## 4. Verify

- Run the task's tests plus the layer's lint/format checks (`docs/workflows/lint-format.md`).
- Check every bullet in the task's **Acceptance criteria** section against what was built; report each as met/not met with evidence.
- If the task has perf implications, note that the `perf-acceptance` skill applies.

## 5. Close the loop

- Check off the task's checkbox in BOTH the epic file and the FEAT file (they are mirrored 1:1). Mark the task's own `**Status:**` line done.
- Link the task file in the PR description; branch name `feat/<task-id>-slug`.
- Commit style: conventional commit with `Task: NN-MMM` footer.
- If you had to make a spec-level decision not covered by PLAN or an ADR, say so in the PR and suggest whether it warrants an ADR.
