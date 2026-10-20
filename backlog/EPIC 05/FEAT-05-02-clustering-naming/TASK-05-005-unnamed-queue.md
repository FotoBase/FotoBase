# task: build unnamed people queue

**Feature:** [FEAT-05-02-clustering-naming](FEAT-05-02-clustering-naming.md) · **Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** todo

## Description
The Unnamed people queue: review unnamed clusters and suggestions with bulk confirm/reject (PLAN §2G "Unnamed people queue"; §6 P3 People).

## Functional specs
- Queue shows unnamed clusters with representative face thumbnails.
- Bulk confirm/reject: apply to all faces in a cluster (or the current suggestion batch) in one action.
- Confirmations write named MWG regions; rejects suppress the suggestion.

## Technical specs
- UI panel in `app/src` (People panel, §2 View menu "Show people"); bulk actions as one catalog+sidecar transaction per face.
- Rejection state stored per face (suggestion suppressed, not deleted — face remains detected).

## Testing criteria (TDD)
- E2e: open queue, bulk-confirm a cluster → all its faces named; bulk-reject another → suggestions gone.
- Integration: bulk confirm → all faces in sidecar MWG regions with the person name.

## Acceptance criteria
- [ ] 05-005: Unnamed queue supports bulk confirm/reject per cluster.
