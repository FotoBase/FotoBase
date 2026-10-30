# task: store recipes in fb:recipe xmp

**Feature:** [FEAT-04-05-edit-recipe-engine](FEAT-04-05-edit-recipe-engine.md) · **Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** in progress (2026-10-30)

## Description
Persist edit recipes in the custom `fb:Recipe` XMP namespace as versioned JSON (PLAN §5), with a migration path for older recipe versions. Recipes must survive catalog rebuild since sidecars are the source of truth for user data (§5 Authority).

## Functional specs
- Edits are visible again after closing/reopening the app and after a full catalog rebuild.
- Older-version recipes migrate forward transparently on read.
- Sidecar writes preserve unknown fields (merge-on-write, §5).

## Technical specs
- `fb:Recipe` custom namespace, versioned JSON payload matching the 00-011 schema.
- Write via exiv2 own bindings in `crates/metadata`; merge-on-write with sidecar backup (§5) — depends on the 00-010 sidecar design doc conventions.
- Migration: version check on read → apply migrations in `crates/core`.

## Testing criteria (TDD)
- Integration: edit a photo → read sidecar → `fb:Recipe` present with correct version and ops JSON.
- Integration: catalog delete + rebuild → recipe restored from sidecar.
- Unit: a recipe with `version: 1` migrates to current version with equivalent output.
- Interop: sidecar round-trips through exiv2 without losing other fields (§9 Interop).

## Acceptance criteria
- [x] 04-002: Recipes stored in `fb:Recipe` XMP and survive catalog rebuild.
- [x] 04-002: Versioning + forward migration path implemented and tested.
