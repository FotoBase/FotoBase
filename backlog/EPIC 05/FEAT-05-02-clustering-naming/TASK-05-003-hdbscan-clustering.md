# task: implement hdbscan face clustering

**Feature:** [FEAT-05-02-clustering-naming](FEAT-05-02-clustering-naming.md) · **Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** todo

## Description
HDBSCAN clustering over face embeddings (`hdbscan`/`linfa` crate or in-house per §4), with cluster persistence in the catalog and incremental re-clustering as new faces arrive (PLAN §6 P3 People; §2G).

## Functional specs
- Detected faces group into clusters of the same person without knowing the person count in advance.
- New faces join existing clusters or form new ones incrementally — no full re-cluster required for every import.
- Clusters persist across restarts.

## Technical specs
- HDBSCAN over embeddings from the sqlite-vec table; if Rust crates fall short, in-house implementation (§4, difficulty accepted).
- Persist cluster assignments in catalog (`faces.person_id` nullable for unnamed clusters, §5 schema); clusters are derived data (recomputable, §5 Authority).
- Incremental: assign new embeddings to nearest existing cluster or spawn a new one.

## Testing criteria (TDD)
- Integration: labeled evaluation set → precision/recall recorded and hitting §9 targets.
- Integration: cluster 100 embeddings, add 10 new → incremental update only touches new faces.
- Unit: same embedding set clusters deterministically across runs.

## Acceptance criteria
- [ ] 05-003: HDBSCAN clustering implemented with cluster persistence and incremental re-clustering.
