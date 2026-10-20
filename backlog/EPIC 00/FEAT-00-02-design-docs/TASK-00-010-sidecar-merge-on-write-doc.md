# task: write sidecar merge-on-write + re-association design doc

**Feature:** [FEAT-00-02-design-docs](FEAT-00-02-design-docs.md) · **Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** done (2026-10-20) → [`docs/sidecar-merge-on-write.md`](../../../docs/sidecar-merge-on-write.md)

## Description
Write the P2a-prerequisite design doc (PLAN §5 "Authority, conflicts & rebuild", §10 sidecar-conflict risk): how Fotobase writes XMP sidecars safely — read-modify-write preserving unknown fields, previous-version backup, hash-based move/rename re-association, and concurrency with digiKam/Syncthing. Hard prerequisite for EPIC-03.

## Functional specs
- Doc specifies: unknown-field preservation on write, previous-version backup (`.originals`-style), hash-based re-association of moved/renamed originals with their sidecars, and expected behavior when another app (digiKam, Syncthing) touches the same sidecar.
- A renamed/moved-outside-app file must re-associate with its sidecar and stars, not be treated as a new photo (§6 P2a acceptance).

## Technical specs
- Covers the `metadata` crate write path (exiv2 bindings, §4) and file locking strategy.
- Re-association keyed on (size, mtime, content hash) per §5.
- Must define the `fb:` custom namespace fields already decided in §11: `fb:StackId`/`fb:StackRole` (Q5), `fb:Hidden` + the folder-level `.fotobase` metadata sidecar (Q6), and `fb:Albums` (§5).
- Doc lives in `docs/`; approved before any EPIC-03 code.

## Testing criteria (TDD)
- Design-level criteria embedded in the doc: a test matrix for merge-on-write (unknown XMP fields survive), backup (previous sidecar recoverable), and re-association (rename/move cases) — these become the EPIC-03 test plan.

## Acceptance criteria
- [x] 00-010: Design doc merged in `docs/` covering unknown-field preservation, backup, hash re-association, and digiKam/Syncthing concurrency.
- [x] EPIC-03 unblocked (doc approved).
