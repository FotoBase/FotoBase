# task: add burst grouping and ai culling

**Feature:** [FEAT-07-01-ai-assists](FEAT-07-01-ai-assists.md) · **Epic:** [EPIC-07 — Smart search](../EPIC-07.md) · **Status:** todo

## Description
Group burst sequences and rank them by sharpness/quality, picking a best shot that feeds EPIC-03 stacking (PLAN §6 P3: "AI culling: burst grouping ranked by sharpness/quality to pick the best shot"; §2 2026 additions). User confirms or overrides the pick.

## Functional specs
- Bursts (photos taken within a short window, same camera/focal length) are grouped and surfaced as a stack candidate with a suggested best shot.
- The suggested best is marked in the UI; the user can promote any shot in the burst with one action.
- User override is user data (sidecar/stack definition from EPIC-03), not recomputed away on rebuild.
- Culling runs as a background job; UI stays responsive.

## Technical specs
- `ai` crate: sharpness/quality score — Laplacian-variance sharpness + exposure/blur heuristics on the decoded preview; no new model needed (keep it cheap; a learned quality model can come later via the same score interface).
- Burst grouping: cluster by `taken_at` gaps below a threshold (e.g. < 2 s) plus same camera from EXIF; runs in the job queue.
- Feeds EPIC-03 stacking: emits stack candidates through the existing stack data path (PLAN §6 P2a "burst stacks grouped under one thumbnail").
- Scores are derived data (catalog only); user's chosen best is user data.

## Testing criteria (TDD)
- Unit: given a synthetic burst timestamp sequence, grouping produces one group per continuous run.
- Integration: fixture burst with one blurred and one sharp frame → sharp frame ranked first and suggested as best.
- Integration: user promotes a different shot → rebuild preserves the user's pick (stack from sidecar/user data, not re-culled).

## Acceptance criteria
- [ ] 07-006: Burst grouping + AI culling ranks by sharpness/quality, picks a best shot feeding EPIC-03 stacking; user confirms or overrides.
- [ ] 07-006: User overrides survive catalog rebuild.
