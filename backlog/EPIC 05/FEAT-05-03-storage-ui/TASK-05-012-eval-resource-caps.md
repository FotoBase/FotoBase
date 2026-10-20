# task: build clustering evaluation and resource-cap tests

**Feature:** [FEAT-05-03-storage-ui](FEAT-05-03-storage-ui.md) · **Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** todo

## Description
Evaluation harness: clustering precision/recall on a labeled face set, plus resource-cap tests for the face pipeline on battery (PLAN §9 AI row; §6 P3 acceptance).

## Functional specs
- CI reports clustering precision/recall numbers against a labeled fixture set — targets recorded, regressions flagged.
- Face pipeline respects resource caps when on battery (throttled, memory ≤ 400 MB).

## Technical specs
- Labeled face fixture set checked in (or generated fixtures with known identities); evaluation computes precision/recall per the §9 AI verification row.
- Resource-cap test: run the pipeline job under a simulated battery state with a memory ceiling assertion.

## Testing criteria (TDD)
- Integration: labeled set → precision/recall computed and compared against recorded baselines.
- Integration: battery-state pipeline run → RSS stays ≤ 400 MB; jobs throttle.

## Acceptance criteria
- [ ] 05-012: Precision/recall evaluation on labeled face set runs in CI with recorded numbers.
- [ ] 05-012: Resource-cap tests on battery pass.
