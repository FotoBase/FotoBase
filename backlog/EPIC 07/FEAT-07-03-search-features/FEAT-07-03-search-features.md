# feat: search features

**Epic:** [EPIC-07 — Smart search](../EPIC-07.md) · **Status:** planned

## Acceptance criteria

- Semantic queries ("dog on the beach at sunset") return relevant results on the 50k fixture, blended with FTS matches (epic acceptance).
- Auto-tags from embeddings become normal `dc:subject` tags, user-confirmable, and survive catalog rebuild as regular tags.
- Field filters (`person:`, `camera:`, `before:`) combine with free text and semantic results in one query syntax.

- [ ] `07-003` (M) Semantic search in the live search bar ("dog on the beach at sunset"), blended with FTS results.
- [ ] `07-004` (M) Auto-tagging: content keywords from embeddings → normal tags (`dc:subject`), user-confirmable.
- [ ] `07-005` (M) Advanced search syntax: `person:`, `camera:`, `before:` field filters combinable with free text.

## Tasks

- [ ] [TASK-07-003-semantic-search-bar.md](TASK-07-003-semantic-search-bar.md) — semantic search blended into live search bar
- [ ] [TASK-07-004-auto-tagging.md](TASK-07-004-auto-tagging.md) — embedding-based auto-tagging
- [ ] [TASK-07-005-field-filter-syntax.md](TASK-07-005-field-filter-syntax.md) — advanced search field filter syntax
