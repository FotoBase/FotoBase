# feat: name & community diligence (§12)

**Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** planned

## Acceptance criteria

- No blocking collisions: GitHub org, crates.io, npm handles claimed and recorded in an ADR.
- Domain registered and parked; whois/registry details stored in the project's private notes.
- Trademark sweep (EUIPO, USPTO, DPMA) documented; if any risk is found, a rename decision is taken _before_ first public commit.
- `CONTRIBUTING.md`, code of conduct, and GPL-3.0 license file exist in the repo; CI license-manifest scaffold runs green.

- [x] `00-005` (S) Check GitHub org, crates.io, npm for "fotobase" collisions; claim handles. Descoped per ADR-0010 — crates.io/npm not needed (GitHub Releases only); create the `fotobase` repo at first public commit.
- [ ] `00-006` (S) Check domain availability (`fotobase.app` / `.dev` / `.io`); register chosen domain. Availability checked 2026-10-20 (ADR-0010: `.app` taken, `.dev`/`.io` free); registration **deferred** until a website exists — not a pre-commit blocker.
- [x] `00-007` (S) Search trademark registries (EUIPO, USPTO, DPMA at minimum); record result in ADR. Clean — go ([ADR-0011](../../../docs/adr/adr-0011-trademark-search.md)).
- [x] `00-008` (S) Draft `CONTRIBUTING.md` + code of conduct (needed at public launch, cheap to do early). Done 2026-10-20.
- [x] `00-009` (S) Choose GPL-3.0 headers + license file; add CI license-manifest scaffolding (per-model license checks arrive with EPIC-05/07). License done; CI scaffold folds into 01-002.

## Tasks

- [x] [TASK-00-005-claim-handles.md](TASK-00-005-claim-handles.md) — check name collisions and claim handles (descoped, ADR-0010)
- [ ] [TASK-00-006-register-domain.md](TASK-00-006-register-domain.md) — check and register the project domain (deferred)
- [x] [TASK-00-007-trademark-search.md](TASK-00-007-trademark-search.md) — search trademark registries (clean, ADR-0011)
- [x] [TASK-00-008-contributing-coc.md](TASK-00-008-contributing-coc.md) — draft contributing guide and code of conduct
- [x] [TASK-00-009-license-manifest.md](TASK-00-009-license-manifest.md) — add GPL-3.0 license and CI license-manifest scaffold
