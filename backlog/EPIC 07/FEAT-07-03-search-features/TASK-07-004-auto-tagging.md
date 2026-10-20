# task: add embedding-based auto-tagging

**Feature:** [FEAT-07-03-search-features](FEAT-07-03-search-features.md) · **Epic:** [EPIC-07 — Smart search](../EPIC-07.md) · **Status:** todo

## Description
Generate content keywords from CLIP embeddings and offer them as normal `dc:subject` tags, user-confirmable (PLAN §6 P3: "Auto-tagging: content keywords generated from embeddings, written as normal tags"). Must survive catalog rebuild as regular tags (epic acceptance, §9).

## Functional specs
- Suggested keywords appear on the photo (info/tag pane) as unconfirmed suggestions, visually distinct from user tags.
- The user confirms/rejects suggestions individually or in bulk; confirmed suggestions become ordinary tags.
- Confirmed tags are written to the XMP sidecar (`dc:subject`, PLAN §5) and are therefore user data — they survive catalog rebuild.
- Unconfirmed suggestions are derived data: not written to sidecars, recomputed on rebuild.
- Suggestion generation only runs after the 07-001 model download opt-in.

## Technical specs
- `ai` crate: keyword extraction — nearest-neighbor of the image embedding against a small fixed vocabulary of prompt embeddings (e.g. "a photo of {concept}"), threshold by cosine distance; keep the vocabulary as a data file so it's extendable without code changes.
- Reuses the 07-002 `embeddings` table and CLIP model; no new model download.
- Tags written via the metadata service (EPIC-03 sidecar write path, merge-on-write per PLAN §5).
- Catalog: `file_tags` rows flagged `confirmed` vs `suggested` so rebuild can restore suggestions without duplicating confirmed tags.

## Testing criteria (TDD)
- Integration: photo of a dog → "dog" appears among suggestions; confirming it writes `dc:subject` to the sidecar.
- Integration (rebuildability): confirm a tag → delete catalog → rebuild → tag still present from sidecar (feeds 07-009).
- Unit: rejecting a suggestion never writes to the sidecar.

## Acceptance criteria
- [ ] 07-004: Auto-tags from embeddings become normal `dc:subject` tags after user confirmation and survive catalog rebuild as regular tags.
- [ ] 07-004: Unconfirmed suggestions never touch sidecars and are recomputable.
