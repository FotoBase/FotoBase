# task: build jobs crate — bounded priority queue with cancellation

**Feature:** [FEAT-01-03-job-queue-thumbnails](FEAT-01-03-job-queue-thumbnails.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** todo

## Description
Implement the `jobs` crate per PLAN §3 rule 4: all heavy work (thumbnailing, metadata extraction) runs in a bounded priority queue — visible items first, background last — with cancellation and a throttling hook (battery throttling arrives later).

## Functional specs
- Visible thumbnails are generated before background ones as the user scrolls.
- The queue is bounded and never exhausts memory on a 50k library.
- A job can be cancelled mid-flight; duplicate jobs for the same photo are coalesced.

## Technical specs
- `tokio` + custom priority queue (§4); priorities: visible-first, background-last.
- Throttling hook is a trait/stub in P1 — battery detection arrives with later epics.
- Cancellation must leave the on-disk cache consistent (no partial thumbnail files).

## Testing criteria (TDD)
- Unit: enqueue N jobs with mixed priorities ⇒ dequeue order is priority-respecting.
- Unit: cancel a running job ⇒ its partial cache entry is removed/ignored; re-enqueue of the same key coalesces with the in-flight job.

## Acceptance criteria
- [ ] 01-009: `jobs` crate with bounded priority queue, cancellation, coalescing, and throttling hook.
