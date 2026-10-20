# task: disconnect/reconnect test harness for network shares

**Feature:** [FEAT-02-02-network-drives](FEAT-02-02-network-drives.md) · **Epic:** [EPIC-02 — Media & platform hardening](../EPIC-02.md) · **Status:** todo

## Description
Provide the repeatable test harness that 02-008/02-009 rely on: a virtual SMB share that can be disconnected/reconnected on demand, in CI where possible, otherwise as a documented manual protocol (PLAN §6 P1.5 acceptance).

## Functional specs
- The harness can mount a share, populate it with a fixture tree, unplug (simulate disconnect), and replug — scripted, repeatable.
- CI runs the pause/resume scenario repeatedly (or the manual protocol is documented step-by-step with expected observations).

## Technical specs
- Prefer a containerized Samba share (e.g. `testcontainers`-style Samba image) mountable in CI runners; macOS runners fall back to the documented manual protocol if SMB-in-CI is not feasible.
- Fixture: small photo tree (tens of files) with known mtimes/sizes.
- Expose helpers: `mount`, `populate`, `disconnect`, `reconnect`, `state` (returns catalog scan state).

## Testing criteria (TDD)
- Integration: full scripted cycle (mount → scan → disconnect → paused → reconnect → complete) passes 3× consecutively in one CI job.
- The manual protocol doc includes the exact pass/fail observations (UI responsive, no dup work, clean resume).

## Acceptance criteria
- [ ] 02-010: harness can disconnect/reconnect a share on demand, scripted.
- [ ] 02-010: pause/resume scenario passes repeatedly.
- [ ] 02-010: CI integration or documented manual protocol exists in the repo.
