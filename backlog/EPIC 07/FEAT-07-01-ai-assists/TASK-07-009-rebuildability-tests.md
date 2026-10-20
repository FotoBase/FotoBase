# task: add smart-search rebuildability tests

**Feature:** [FEAT-07-01-ai-assists](FEAT-07-01-ai-assists.md) · **Epic:** [EPIC-07 — Smart search](../EPIC-07.md) · **Status:** todo

## Description
Verify per PLAN §9 (Rebuildability: "delete catalog → rebuild → diff all user data against pre-delete snapshot") that embeddings are recomputable and confirmed auto-tags survive as regular tags. This is the epic's second acceptance criterion made executable.

## Functional specs
- After a full catalog rebuild, confirmed auto-tags are still present (they live in sidecars as `dc:subject`).
- Semantic search works again after rebuild without user action beyond the rebuild itself.
- No duplicate tags are created by rebuild + re-suggestion.

## Technical specs
- Test harness in the Rust integration suite (PLAN §4 Testing: `cargo test`), reusing the 50k fixture infrastructure from EPIC-01.
- Scenario: snapshot user data → run embedding + auto-tag flow → confirm a tag → delete catalog DB → rebuild from sidecars (incremental + full) → diff sidecars and query results against snapshot.
- Covers the authority split (PLAN §5): derived data recomputed, user data re-read from sidecars.

## Testing criteria (TDD)
- Integration: rebuild after confirming auto-tags → tags present exactly once, sourced from sidecars.
- Integration: rebuild → embeddings table fully repopulated and semantic query returns the same ranked set (within tolerance for nondeterministic ANN ordering).
- Regression: this test fails if any smart-search feature starts writing derived data into sidecars.

## Acceptance criteria
- [ ] 07-009: Rebuildability tests prove embeddings recomputable and auto-tags survive catalog rebuild as regular tags (§9).
- [ ] 07-009: Rebuild creates no duplicate tags.
