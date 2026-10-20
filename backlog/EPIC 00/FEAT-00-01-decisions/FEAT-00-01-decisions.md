# feat: decisions (§11)

**Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** planned

## Acceptance criteria

- All four §11 decisions are recorded in ADRs under `docs/adr/` with date, status, context, and consequences.
- Each ADR is linked from PLAN.md §11 so the plan and the decision log never drift apart.
- New decisions follow the same format without needing to ask how to write one up.

- [x] `00-001` (S) **Resolved 2026-10-20:** single star by default, 1–5 ratings as opt-in setting; both map to `xmp:Rating`. Recorded in PLAN §11 Q1.
- [x] `00-002` (S) **Resolved 2026-10-20:** sidecars only by default, opt-in embed on Save. Recorded in PLAN §11 Q2.
- [x] `00-003` (S) **Resolved 2026-10-20:** Windows 10 22H2+, macOS 12+, Ubuntu 22.04+. Recorded in PLAN §1 + §11 Q4; CI matrix (01-002) uses these floors.
- [x] `00-004` (S) Record decision log format (ADR-style) in `docs/adr/`; seed with the 2026-10-20 decisions already in PLAN. Done: [adr-0001…0009](../../../docs/adr/).

## Tasks

- [x] [TASK-00-001-single-star-rating.md](TASK-00-001-single-star-rating.md) — decide single star vs 1–5 ratings, map to xmp:Rating
- [x] [TASK-00-002-sidecar-only-default.md](TASK-00-002-sidecar-only-default.md) — decide sidecar-only metadata with opt-in embed on save
- [x] [TASK-00-003-os-version-floors.md](TASK-00-003-os-version-floors.md) — set minimum OS version floors
- [x] [TASK-00-004-adr-decision-log.md](TASK-00-004-adr-decision-log.md) — set up ADR-style decision log and seed it
