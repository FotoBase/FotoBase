# task: run face detection and embedding as throttled jobs

**Feature:** [FEAT-05-01-ai-runtime-models](FEAT-05-01-ai-runtime-models.md) · **Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** todo

## Description
Face detection (YuNet) and embedding extraction run as job-queue jobs honoring battery throttling, keeping the UI responsive during a full-library face pass (PLAN §6 P3 People; §3 rule 4).

## Functional specs
- A full-library face pass runs in the background; UI stays responsive.
- On battery, the face pipeline throttles/pauses per the job queue's battery rule (§10 "pause on metered/battery").
- Detection runs per folder honoring the per-folder face-detection toggle (05-010).

## Technical specs
- Jobs in the bounded priority queue (`crates/jobs`): detect → crop face region → embed, results into catalog `faces` + `embeddings` tables (§5).
- Per-folder toggle read from Folder Manager settings before scheduling.
- Detection and embedding are separate job kinds so embeds can skip already-processed faces.

## Testing criteria (TDD)
- Integration: 10k fixture → all faces detected/embedded overnight budget, ≤ 400 MB RAM (epic acceptance, §9 AI).
- Integration: simulated battery state → jobs throttle; AC state → full speed.
- Unit: folder with detection disabled → no face jobs scheduled.

## Acceptance criteria
- [ ] 05-002: Detection + embedding run as job-queue jobs with battery throttling hook.
- [ ] 05-002: UI responsive during full-library face pass.
