# Contributing to Fotobase

Thanks for your interest in Fotobase — a lightweight, offline, cross-platform photo app for people with big libraries on old hardware ([docs/PLAN.md](docs/PLAN.md) has the full vision).

## Getting started

1. Rust (stable) + Node LTS + Tauri v2 prerequisites for your OS.
2. Python 3 + Pillow if you touch the perf fixture: `python3 tools/fixture_gen.py --count 1000 --seed 42 --out /tmp/lib`.
3. Clone, then build and test per the instructions that arrive with the app skeleton (EPIC-01).

## Ground rules (from PLAN §3)

- **Originals are read-only** except explicit "Save". Never write to a user's photo files as a side effect.
- **XMP sidecars are the source of truth** for user data; the SQLite catalog is a rebuildable cache. Never write derived data back to sidecars.
- **No images over IPC** — only `thumb://` / `preview://` URIs.
- **One edit-recipe engine** for preview and export; ops are pure functions over a linear-light buffer ([recipe spec](docs/edit-recipe-spec-v1.md)).
- Sidecar writes follow [docs/sidecar-merge-on-write.md](docs/sidecar-merge-on-write.md) — this is the highest data-loss-risk area; read it before touching `crates/metadata`.

## Workflow

- One task ≈ one PR. Tasks live in `backlog/EPIC XX/FEAT-XX-YY-name/TASK-*.md` — each has functional specs, technical specs, TDD testing criteria, and acceptance criteria. Work test-first: make the failing test, then implement.
- Branch naming: `feat/<task-id>-slug`, `fix/<task-id>-slug`.
- Keep PRs small and mapped to a backlog task; link the task file in the PR description.
- Tests must pass on all three OSes (Windows 10 22H2+, macOS 12+, Ubuntu 22.04+) — CI runs the matrix.

## Decisions

Architecture and product decisions are recorded as ADRs in [`docs/adr/`](docs/adr/) (see [ADR-0001](docs/adr/adr-0001-purpose-and-ground-rules.md) for the format). If your change contradicts an ADR, open an issue first.

## Licensing

By contributing, you agree your contributions are licensed under [GPL-3.0](LICENSE). Model/asset licenses are tracked per-component (AI models arrive in EPIC-05/07 with their own manifest checks).

## Code of conduct

This project follows the [Contributor Covenant v2.1](CODE_OF_CONDUCT.md). Enforcement contacts are announced at public launch.
