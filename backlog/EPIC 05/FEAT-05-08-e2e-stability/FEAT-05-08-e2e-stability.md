# feat: e2e suite stability

**Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** planned

## Acceptance criteria

- The e2e suite gates merges again: the previously-flaky specs (`edit-view.e2e.ts`, `tray.e2e.ts`) pass 3 consecutive CI runs, the failure rate before/after the fix is recorded in the PR, and no assertion is weakened.
- Flaky failures are diagnosable from the CI log alone (which phase failed, which attempt, why).

- [x] `05-013` (M) Stabilize the e2e suite: edit-view interaction hangs under xvfb, tray-quit drain race, session-creation port race.

## Tasks

- [x] [TASK-05-013-e2e-stability.md](TASK-05-013-e2e-stability.md) — stabilize the e2e suite (edit-view hangs, tray quit race)
