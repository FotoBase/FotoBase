# feat: clustering & naming

**Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** planned

## Acceptance criteria

- HDBSCAN clustering on the labeled evaluation set hits the precision/recall targets recorded in §9; clusters persist and re-cluster incrementally as new faces arrive.
- Name-a-cluster assigns a person in one flow; remaining matches get suggestions; unnamed queue supports bulk confirm/reject.
- People Manager lists all named + unnamed clusters; person albums auto-created; "has faces" filter and name search work in FTS.
- Full pipeline on 10k photos completes overnight on a low-end laptop, ≤ 400 MB RAM (epic acceptance).

- [ ] `05-003` (L) HDBSCAN clustering (crate or in-house per §4); cluster persistence; re-cluster incrementally.
- [ ] `05-004` (M) Name-a-cluster flow; suggestions for remaining matches.
- [ ] `05-005` (M) Unnamed people queue: bulk confirm/reject.
- [ ] `05-006` (M) People Manager dialog (all named + unnamed clusters).
- [ ] `05-007` (M) Person albums (auto album per person); "has faces" filter; names in FTS search.

## Tasks

- [ ] [TASK-05-003-hdbscan-clustering.md](TASK-05-003-hdbscan-clustering.md) — implement hdbscan face clustering
- [ ] [TASK-05-004-name-cluster-flow.md](TASK-05-004-name-cluster-flow.md) — implement name-a-cluster flow with suggestions
- [ ] [TASK-05-005-unnamed-queue.md](TASK-05-005-unnamed-queue.md) — build unnamed people queue
- [ ] [TASK-05-006-people-manager.md](TASK-05-006-people-manager.md) — build people manager dialog
- [ ] [TASK-05-007-person-albums-filters.md](TASK-05-007-person-albums-filters.md) — implement person albums and face filters
