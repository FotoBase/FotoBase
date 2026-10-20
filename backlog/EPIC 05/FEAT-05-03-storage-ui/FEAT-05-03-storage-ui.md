# feat: storage & UI

**Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** planned

## Acceptance criteria

- Face regions persist as MWG `mwg-rs:Regions` XMP; named faces survive catalog rebuild.
- Embeddings stored in sqlite-vec; "Reset faces" per photo and per-folder detection toggle behave as labeled.
- Contacts link maps names to local contacts only — no network calls, no telemetry.
- Precision/recall evaluation + on-battery resource-cap tests pass in CI (§9).

- [ ] `05-008` (M) Face regions → MWG `mwg-rs:Regions` XMP write; sqlite-vec embeddings table.
- [ ] `05-009` (S) "Reset faces" per photo (picture context menu).
- [ ] `05-010` (S) Per-folder face-detection toggle in Folder Manager.
- [ ] `05-011` (M) Contacts link (local contacts only) — name ↔ contact mapping.
- [ ] `05-012` (M) Clustering precision/recall evaluation on labeled face set (§9); resource-cap tests on battery.

## Tasks

- [ ] [TASK-05-008-mwg-regions-embeddings.md](TASK-05-008-mwg-regions-embeddings.md) — persist face regions as mwg xmp with sqlite-vec embeddings
- [ ] [TASK-05-009-reset-faces.md](TASK-05-009-reset-faces.md) — add reset faces action
- [ ] [TASK-05-010-folder-face-toggle.md](TASK-05-010-folder-face-toggle.md) — add per-folder face-detection toggle
- [ ] [TASK-05-011-contacts-link.md](TASK-05-011-contacts-link.md) — implement local contacts link
- [ ] [TASK-05-012-eval-resource-caps.md](TASK-05-012-eval-resource-caps.md) — build clustering evaluation and resource-cap tests
