# task: blend semantic search into live search bar

**Feature:** [FEAT-07-03-search-features](FEAT-07-03-search-features.md) · **Epic:** [EPIC-07 — Smart search](../EPIC-07.md) · **Status:** todo

## Description
Wire the semantic ANN path (07-002) into the existing live search bar so free-text queries blend FTS5 matches with CLIP semantic results (PLAN §6 P3: "Semantic search in the live search bar"). Epic acceptance: relevant results for common queries on the 50k fixture.

## Functional specs
- Typing "dog on the beach at sunset" returns semantically relevant photos even with no matching filename/tag/caption.
- FTS matches (filename, folder, caption, tags) rank alongside semantic hits; exact text matches are not lost in blending.
- If the model isn't downloaded, search still works as plain FTS with a hint on how to enable semantic search.
- Results update live while typing, same as the P1 search bar.

## Technical specs
- `catalog` crate: merge FTS5 results (`files_fts`) with sqlite-vec ANN results — reciprocal-rank or score-normalized blend; keep the blend deterministic and unit-testable.
- UI (Svelte): no new input surface — reuse the existing search bar component; add a small "semantic" indicator when the CLIP path contributed results.
- Debounce semantic queries (they're heavier than FTS); cache the last query's encoded text vector.

## Testing criteria (TDD)
- Unit: blend function given FTS ranks [a,b] and ANN ranks [b,c] produces a deterministic merged ordering.
- Integration: on the 50k fixture, "dog on the beach at sunset" returns the known-relevant fixture photos in the top results (fixture has labeled ground truth).
- E2E: typing in the search bar updates the grid without a model downloaded (FTS-only fallback).

## Acceptance criteria
- [ ] 07-003: Semantic queries in the live search bar return relevant results on the 50k fixture, blended with FTS matches (epic acceptance).
- [ ] 07-003: Search degrades gracefully to FTS-only when the model is not downloaded.
