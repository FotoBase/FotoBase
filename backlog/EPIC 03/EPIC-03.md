# EPIC-03 — Organize (sidecars, albums, search)

**Plan ref:** §6 Phase 2a · **Size:** L · **Depends on:** EPIC-01; **hard gate satisfied 2026-10-20:** [00-010 design doc](../../docs/sidecar-merge-on-write.md)

## Acceptance (from PLAN §6 P2a)

- Originals unchanged (checksum) after an edit session.
- Delete catalog → rebuild → no loss of stars/tags/captions/albums/edits.
- darktable/digiKam show tags, ratings, captions from our sidecars.
- File renamed/moved outside Fotobase re-associates with sidecar and stars (hash match), not treated as new.

## feat: XMP sidecar layer

- [x] `03-001` (L) `metadata` crate: exiv2 own bindings (rexiv2 fork); read/write sidecars; merge-on-write per 00-010 (preserve unknown fields, backup previous version).
- [x] `03-002` (M) Sidecar sync engine: catalog ↔ sidecar authority rules (sidecar wins for user data, §5); write-through on every user-data mutation.
- [x] `03-003` (M) Move/rename re-association: (size, mtime, hash) matching; sidecar follows the original (00-010 design).
- [x] `03-004` (M) Interop test suite: round-trip sidecars through darktable + digiKam (tags, ratings, captions, GPS) in CI.

## feat: user organization data

- [x] `03-005` (M) Star/rating (per 00-001 decision) → `xmp:Rating`; starred filter.
- [x] `03-006` (M) Tags/keywords → `dc:subject` (+ `lr:hierarchicalSubject`); tag editor; Quick Tags panel (10 one-click buttons).
- [x] `03-007` (M) Captions → `dc:description`; searchable.
- [x] `03-008` (M) Albums: create (name/date/place/description), delete, add/remove photos; `fb:Albums` namespace; album list UI.
- [x] `03-009` (S) Hidden folders + hidden photos (library only, files untouched).

## feat: photo tray & batch

- [x] `03-010` (M) Photo Tray: cross-folder selection, "Hold" pinning, actions apply to tray.
- [x] `03-011` (M) Batch rename to pattern; batch move between folders (real file moves, catalog follows).
- [x] `03-012` (M) Catalog trash: soft-delete + undo window for remove-from-album, hide, unlink.

## feat: search & filters

- [x] `03-013` (M) Extend FTS to captions + tags; live search covers them.
- [x] `03-014` (M) Filter buttons: starred, video; date slider (last N days/months).

## feat: view options

- [x] `03-015` (M) Thumbnail captions (none/filename/caption/tags/resolution).
- [x] `03-016` (S) Folder sort (name/date/size, reverse), flat vs tree view.
- [ ] `03-017` (S) Single-click-to-edit option (General options).
- [ ] `03-018` (S) View-time display modes: sepia, B&W (no edit applied).

## feat: metadata editing & stacking

- [ ] `03-019` (M) Date/time adjustment: shift capture time, fix timezones, merge multi-camera sets.
- [ ] `03-020` (M) Metadata editing in info pane: date, GPS, artist, copyright.
- [ ] `03-021` (M) Stacking: RAW+JPEG pairs, burst stacks, edited versions under one thumbnail.

## feat: Picasa migration & portability

- [ ] `03-022` (M) Picasa.ini ingest on first scan: legacy stars, albums, faces → XMP ("your old library, restored").
- [ ] `03-023` (M) Portable export: albums + tags + recipes to JSON/CSV (§5 anti-lock-in).
- [ ] `03-024` (M) Optional multiple catalogs (work/personal); catalog switcher UI.

## feat: OS integration

- [ ] `03-025` (S) Tray icon for background watching.
- [ ] `03-026` (M) Rebuild acceptance harness: delete catalog → rebuild → diff user data vs snapshot (§9).
