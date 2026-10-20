# EPIC-10 — Future features (F1–F5)

**Plan ref:** §7 · **Status:** coarse by design — each item is refined into features/tasks only when it enters an active phase. Ordering per §7 Phase 6 ordering.

## F1 — Sync & web albums

- **F1.1 Publish web album** (after EPIC-08; earliest of the futures, best demo artifact — ship early):
  - [ ] `F1-001` (M) Static HTML gallery generator (resized images + tiny JS viewer) for an album.
  - [ ] `F1-002` (M) Upload to any export target under unguessable URL; incremental re-publish (= "Sync to Web").
- **F1.2 Device sync** (after stable XMP model, EPIC-03; before/with EPIC-09 usefulness):
  - [ ] `F1-003` (XL) Metadata as CRDT document (Loro/Automerge); only confirmed face names sync.
  - [ ] `F1-004` (L) iroh transport (QUIC, relay fallback), QR pairing; originals not synced by default.
- **F1.3 Self-hosted server** (only on demand): axum + SQLite + S3 always-on peer, private albums with login.

## F2 — Advanced editor (after EPIC-04 recipe engine)

- [ ] `F2-001` (M) External editor round-trip (GIMP/Photoshop/Affinity/darktable); result auto-imports as stacked version.
- [ ] `F2-002` (L) Pro tab: curves, levels, HSL, exposure, vibrance, perspective/keystone, Lensfun lens correction, noise reduction.
- [ ] `F2-003` (XL) GPU pipeline: recipe ops ported to `wgpu` shaders, real-time full-res editing.
- [ ] `F2-004` (XL) On-device AI tools: MobileSAM/EfficientSAM masks, LaMa magic erase, AI denoise.

## F3 — Live sharing (after F1.2)

- Shared albums (invite-by-link, E2E encrypted per-album key), view-together sessions (iroh-gossip), comments/reactions. Scope guard: link out for chat.

## F4 — Portable gift export (after EPIC-04; cheapest, first in ordering)

- [ ] `F4-001` (M) Export to folder/zip/USB with offline HTML viewer (reuses F1.1 generator); full-res, captions, slideshow with music.
- [ ] `F4-002` (M) PDF photo book with auto grid layouts, print-shop ready.

## F5 — Plugin ecosystem

- Starts only when ≥2 real plugins exist (EPIC-08 ships several): trait + manifest + sandboxing; metadata importers (Lightroom, Apple Photos), effect preset packs (home of the cut Picasa novelty effects), gallery themes.
- [ ] `F5-001` (M) Plugin manifest + loading trait (importer, preset pack, gallery theme).
- [ ] `F5-002` (M) Plugin sandboxing with fault isolation — broken plugin can't crash or corrupt the catalog.
- [ ] `F5-003` (M) At least one external plugin loads and runs from outside the tree as proof of the API.
