# feat: design docs that gate later epics

**Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** planned

## Acceptance criteria

- Sidecar merge-on-write doc (00-010) approved and merged; EPIC-03 unblocked. Doc specifies: unknown-field preservation, previous-version backup, hash-based re-association, and digiKam/Syncthing concurrency behavior.
- Edit-recipe JSON spec v1 (00-011) published in `docs/` with a JSON Schema; a third party could write a compatible recipe reader from the spec alone.
- WebKitGTK benchmark protocol (00-012) defines the fixture, the measured metrics, and the numeric perf bar that triggers the canvas grid; runnable end-to-end on ubuntu-latest.

- [x] `00-010` (M) **Sidecar merge-on-write + move/rename re-association design doc** (§5) — read-modify-write preserving unknown fields, previous-version backup, hash-based re-association, concurrency with digiKam/Syncthing. **Hard prerequisite for EPIC-03.** Done 2026-10-20: [`docs/sidecar-merge-on-write.md`](../../../docs/sidecar-merge-on-write.md).
- [x] `00-011` (M) Edit-recipe JSON spec v1 + JSON Schema published in `docs/` (§5 anti-lock-in; needed before EPIC-04 ops freeze). Done 2026-10-20: [`docs/edit-recipe-spec-v1.md`](../../../docs/edit-recipe-spec-v1.md).
- [x] `00-012` (S) WebKitGTK perf benchmark protocol: 50k-fixture, define the perf bar that triggers the canvas grid (§10 risk 1). Runs as EPIC-01 entry gate. Done 2026-10-20: [`docs/webkitgtk-perf-protocol.md`](../../../docs/webkitgtk-perf-protocol.md).

## Tasks

- [x] [TASK-00-010-sidecar-merge-on-write-doc.md](TASK-00-010-sidecar-merge-on-write-doc.md) — write sidecar merge-on-write + re-association design doc
- [x] [TASK-00-011-edit-recipe-spec.md](TASK-00-011-edit-recipe-spec.md) — publish edit-recipe JSON spec v1 + schema
- [x] [TASK-00-012-webkitgtk-benchmark-protocol.md](TASK-00-012-webkitgtk-benchmark-protocol.md) — define WebKitGTK perf benchmark protocol
