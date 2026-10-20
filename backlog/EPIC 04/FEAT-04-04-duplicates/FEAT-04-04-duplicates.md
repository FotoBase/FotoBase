# feat: duplicates

**Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** planned

## Acceptance criteria

- Perceptual-hash finder groups near-duplicates across the catalog with a review UI (keep / suggest-delete); nothing is deleted without explicit user action.
- Hashing runs as a background job on the 50k fixture without blocking the UI.
- Known-duplicate fixture set achieves full recall with a tolerable false-positive rate (recorded numbers).

- [ ] `04-019` (M) Near-duplicate finder: perceptual hash (`image_hasher`) across catalog; review UI with keep/suggest-delete.

## Tasks

- [ ] [TASK-04-019-near-duplicate-finder.md](TASK-04-019-near-duplicate-finder.md) — build near-duplicate finder
