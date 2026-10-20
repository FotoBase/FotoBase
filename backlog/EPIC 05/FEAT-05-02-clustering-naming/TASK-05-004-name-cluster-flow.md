# task: implement name-a-cluster flow with suggestions

**Feature:** [FEAT-05-02-clustering-naming](FEAT-05-02-clustering-naming.md) · **Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** todo

## Description
Name a cluster once and get suggestions for remaining matches across the library (PLAN §2G Naming; §6 P3 People).

## Functional specs
- Naming a cluster creates a person; the name is offered as a suggestion on similar unmatched faces.
- Suggestions appear on photos with unconfirmed faces; user confirms/rejects individually.
- Confirmed names are written as named MWG face regions (05-008) so they survive rebuild.

## Technical specs
- Person record in catalog `people` table; confirmed faces write name to `mwg-rs:Regions` via `crates/metadata` (§5).
- Suggestion engine: nearest clusters/embeddings by cosine similarity with a confidence threshold.

## Testing criteria (TDD)
- Integration: name a cluster → remaining high-similarity faces surface as suggestions with the correct name.
- Integration: confirm a suggestion → name present in sidecar MWG region; survives catalog rebuild.

## Acceptance criteria
- [ ] 05-004: Name-a-cluster flow assigns a person in one flow; remaining matches get suggestions.
