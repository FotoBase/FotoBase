# EPIC-05 — People (face recognition)

**Plan ref:** §6 Phase 3, People track · **Size:** XL (track, parallel with 06/07) · **Depends on:** EPIC-03

## Acceptance

- Face pipeline on 10k photos completes overnight on a low-end laptop, ≤ 400 MB RAM.
- Named faces survive catalog rebuild via MWG regions.

## feat: AI runtime & models

- [ ] `05-001` (L) `ai` crate: `ort` (ONNX Runtime) integration, CPU default; opt-in model download flow (license manifest checked in CI — YuNet detect MIT, MobileFaceNet-class embed Apache-2.0, per §4 pins).
- [ ] `05-002` (M) Face detection (YuNet) + embedding as job-queue jobs; battery throttling hook.

## feat: clustering & naming

- [ ] `05-003` (L) HDBSCAN clustering (crate or in-house per §4); cluster persistence; re-cluster incrementally.
- [ ] `05-004` (M) Name-a-cluster flow; suggestions for remaining matches.
- [ ] `05-005` (M) Unnamed people queue: bulk confirm/reject.
- [ ] `05-006` (M) People Manager dialog (all named + unnamed clusters).
- [ ] `05-007` (M) Person albums (auto album per person); "has faces" filter; names in FTS search.

## feat: storage & UI

- [ ] `05-008` (M) Face regions → MWG `mwg-rs:Regions` XMP write; sqlite-vec embeddings table.
- [ ] `05-009` (S) "Reset faces" per photo (picture context menu).
- [ ] `05-010` (S) Per-folder face-detection toggle in Folder Manager.
- [ ] `05-011` (M) Contacts link (local contacts only) — name ↔ contact mapping.
- [ ] `05-012` (M) Clustering precision/recall evaluation on labeled face set (§9); resource-cap tests on battery.
