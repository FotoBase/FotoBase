# EPIC-00 — Foundation & decisions

**Plan ref:** §11, §12 · **Size:** M · **Depends on:** —

Everything that must land before or alongside P1 code: blocking decisions, name diligence, and the P2a prerequisite design doc (tracked here because it gates EPIC-03).

## Acceptance

- All four §11 open questions resolved and recorded in PLAN.md.
- Name diligence checklist done before first public commit.
- Sidecar merge-on-write design doc approved (gates EPIC-03).

## feat: decisions (§11)

- [x] `00-001` (S) **Resolved 2026-10-20:** single star by default, 1–5 ratings as opt-in setting; both map to `xmp:Rating`. Recorded in PLAN §11 Q1.
- [x] `00-002` (S) **Resolved 2026-10-20:** sidecars only by default, opt-in embed on Save. Recorded in PLAN §11 Q2.
- [x] `00-003` (S) **Resolved 2026-10-20:** Windows 10 22H2+, macOS 12+, Ubuntu 22.04+. Recorded in PLAN §1 + §11 Q4; CI matrix (01-002) uses these floors.
- [x] `00-004` (S) Record decision log format (ADR-style) in `docs/adr/`; seed with the 2026-10-20 decisions already in PLAN. Done 2026-10-20: [adr-0001…0009](../../docs/adr/).

## feat: name & community diligence (§12)

- [x] `00-005` (S) Check GitHub org, crates.io, npm for "fotobase" collisions; claim handles. Descoped 2026-10-20 (ADR-0010): GitHub Releases-only distribution means crates.io/npm claims aren't needed (both verified free); a named repo under the owner's account claims the GitHub name — create `fotobase` repo at first public commit.
- [ ] `00-006` (S) Check domain availability (`fotobase.app` / `.dev` / `.io`); register chosen domain. Availability checked 2026-10-20 (ADR-0010: `.app` taken, `.dev`/`.io` free); registration **deferred** until a website exists — not a pre-commit blocker.
- [x] `00-007` (S) Search trademark registries (EUIPO, USPTO, DPMA at minimum); record result in ADR. Done 2026-10-20: clean, go on the name ([ADR-0011](../../docs/adr/adr-0011-trademark-search.md)).
- [x] `00-008` (S) Draft `CONTRIBUTING.md` + code of conduct (needed at public launch, cheap to do early). Done 2026-10-20: [CONTRIBUTING.md](../../CONTRIBUTING.md) + [CODE_OF_CONDUCT.md](../../CODE_OF_CONDUCT.md) (Covenant v2.1).
- [x] `00-009` (S) Choose GPL-3.0 headers + license file; add CI license-manifest scaffolding (per-model license checks arrive with EPIC-05/07). License file done 2026-10-20 ([LICENSE](../../LICENSE), canonical GPL-3.0); CI scaffold folds into 01-002 when CI exists.

## feat: design docs that gate later epics

- [x] `00-010` (M) **Sidecar merge-on-write + move/rename re-association design doc** (§5) — read-modify-write preserving unknown fields, previous-version backup, hash-based re-association, concurrency with digiKam/Syncthing. **Hard prerequisite for EPIC-03.** Done 2026-10-20: [`docs/sidecar-merge-on-write.md`](../../docs/sidecar-merge-on-write.md).
- [x] `00-011` (M) Edit-recipe JSON spec v1 + JSON Schema published in `docs/` (§5 anti-lock-in; needed before EPIC-04 ops freeze). Done 2026-10-20: [`docs/edit-recipe-spec-v1.md`](../../docs/edit-recipe-spec-v1.md) + [`recipe-v1.schema.json`](../../docs/recipe-v1.schema.json).
- [x] `00-012` (S) WebKitGTK perf benchmark protocol: 50k-fixture, define the perf bar that triggers the canvas grid (§10 risk 1). Runs as EPIC-01 entry gate. Done 2026-10-20: [`docs/webkitgtk-perf-protocol.md`](../../docs/webkitgtk-perf-protocol.md).

## feat: performance fixture

- [x] `00-013` (M) Build the 50k-photo test fixture generator (mixed formats, dates, folder depth; scriptable, committed as generator not data). Used by every epic's acceptance tests (§9). Done 2026-10-20: [tools/fixture_gen.py](../../tools/fixture_gen.py) + HEIC samples per ADR-0009; determinism verified (same seed ⇒ identical sha256 manifest).
