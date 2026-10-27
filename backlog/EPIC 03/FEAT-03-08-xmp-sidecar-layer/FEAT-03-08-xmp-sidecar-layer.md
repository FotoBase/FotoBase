# feat: XMP sidecar layer

**Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** planned · **Hard gate satisfied 2026-10-20:** [00-010 design doc](../../../docs/sidecar-merge-on-write.md)

## Acceptance criteria

- Merge-on-write preserves fields written by other apps: a digiKam edit re-read after our write keeps its data; previous sidecar version is backed up before every write.
- Sidecar is authoritative for user data: catalog ↔ sidecar sync converges after any external change; delete catalog → rebuild → zero loss (starred/tags/captions/albums survive).
- File renamed/moved outside Fotobase re-associates with its sidecar and stars via (size, mtime, hash) matching — not treated as new.
- Interop suite round-trips tags, ratings, captions, GPS through darktable and digiKam in CI.

- [x] `03-001` (L) `metadata` crate: exiv2 own bindings (rexiv2 fork); read/write sidecars; merge-on-write per 00-010 (preserve unknown fields, backup previous version).
- [x] `03-002` (M) Sidecar sync engine: catalog ↔ sidecar authority rules (sidecar wins for user data, §5); write-through on every user-data mutation.
- [x] `03-003` (M) Move/rename re-association: (size, mtime, hash) matching; sidecar follows the original (00-010 design).
- [ ] `03-004` (M) Interop test suite: round-trip sidecars through darktable + digiKam (tags, ratings, captions, GPS) in CI.

## Tasks

- [x] [TASK-03-001-metadata-crate-sidecars.md](TASK-03-001-metadata-crate-sidecars.md) — metadata crate: own exiv2 bindings and sidecar read/write
- [x] [TASK-03-002-sidecar-sync-engine.md](TASK-03-002-sidecar-sync-engine.md) — sidecar sync engine: catalog ↔ sidecar authority
- [ ] [TASK-03-003-move-rename-reassociation.md](TASK-03-003-move-rename-reassociation.md) — move/rename re-association by (size, mtime, hash)
- [ ] [TASK-03-004-interop-test-suite.md](TASK-03-004-interop-test-suite.md) — darktable/digiKam interop test suite in CI
