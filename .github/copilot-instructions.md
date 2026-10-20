---
applyTo: "**"
---

# Fotobase — ground rules for agents

Fotobase is a local-first, cross-platform (Win 10 22H2+ / macOS 12+ / Ubuntu 22.04+), open-source Picasa successor photo organizer. GPL-3.0. The normative plan is `docs/PLAN.md`; decisions are recorded in `docs/adr/`. If a change contradicts an accepted ADR, it must first be superseded by a new ADR.

## Communication — mandatory

Always use the caveman skill (`/caveman`, default intensity) for every response in this repo. Mandatory, no exceptions: ultra-compressed output, full technical accuracy.

## Architecture invariants (PLAN §3 — never violate)

1. Originals are read-only to the app except on explicit "Save".
2. XMP sidecars are the source of truth for user data. The SQLite catalog is a rebuildable cache. Never write derived data back to sidecars.
3. Images never travel over IPC. Thumbnails/previews are served via the custom Tauri URI schemes `thumb://` / `preview://` from disk cache.
4. All heavy work runs in a bounded job queue with priorities (visible items first); throttle on battery.
5. One edit-recipe engine for preview and export; ops are pure functions over a linear-light buffer (see `docs/edit-recipe-spec-v1.md`).

## Sidecar safety — highest data-loss-risk area

Before touching anything that writes metadata, read `docs/sidecar-merge-on-write.md`. It is normative in full: `fb:` namespace `https://fotobase.github.io/ns/1.0/`, read-modify-write merge that preserves all unknown nodes verbatim, atomic replace via `.tmp` + rename, `.fb-bak` backup, sidecar-wins conflict rule, catalog updated only after the sidecar write succeeds. A Fotobase write may never destroy XMP content it does not understand.

## Stack (pinned — do not add alternatives)

- Rust core in a Cargo workspace: `crates/{core, catalog, scanner, metadata, render, ai, export, video, jobs}` + `app/src-tauri` (Tauri v2) + `app/src` (Svelte 5 + TypeScript + Vite).
- Plain CSS / CSS variables — no CSS framework. TanStack Virtual, MapLibre.
- SQLite (`rusqlite` + FTS5 + sqlite-vec); exiv2 via own bindings; libvips/zune-jpeg/LibRaw/libheif decode; `ort` ONNX for AI; ffmpeg for video.
- Testing: `cargo test` + `insta` snapshots, golden-image tests with per-pixel tolerance, Vitest + Testing Library, WebdriverIO with `tauri-driver`.
- No telemetry of any kind. Permissively-licensed AI models only (license manifest checked in CI). GPL-3.0.

## Hard performance budgets (PLAN §1, §6)

- < 150 MB idle RAM on a 50k-photo library
- < 20 MB installer
- First thumbnails visible < 2 s after scan start (50k photos)
- 60 fps scroll on all three OSes (incl. WebKitGTK — DOM grid or canvas fallback, whichever passes)
- Protocol + report schema: `docs/webkitgtk-perf-protocol.md`

## Backlog conventions

- One task ≈ one PR. Branches: `feat/<task-id>-slug`, `fix/<task-id>-slug` (e.g. `feat/01-014-clap-di`).
- Tasks live in `backlog/EPIC NN/FEAT-NN-MM-name/TASK-NN-MMM-slug.md` with sections: Description / Functional specs / Technical specs / Testing criteria (TDD) / Acceptance criteria, each citing PLAN §refs.
- Task IDs look like `01-001`; epics and FEAT files carry mirrored checkbox lines `- [ ] \`NN-MMM\` (S|M|L) …`. When a task is done, check its box in BOTH the epic and the FEAT file.
- Work test-first: make the failing test, then implement (see the `implement-task` skill).

## Normative docs map

| Doc                                                     | Governs                                                                              |
| ------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| `docs/PLAN.md`                                          | Phases, budgets, architecture, acceptance criteria                                   |
| `docs/adr/adr-0001…0012`                                | Accepted decisions (ADR-0010: GitHub Releases only; ADR-0012: versioning & releases) |
| `docs/edit-recipe-spec-v1.md` + `recipe-v1.schema.json` | Edit recipe JSON (versioned, unknown-op-skip)                                        |
| `docs/sidecar-merge-on-write.md`                        | All XMP sidecar writes                                                               |
| `docs/webkitgtk-perf-protocol.md`                       | Perf runs, `perf-report.json` schema, DOM-vs-canvas decision                         |
| `docs/workflows/release-checklist.md`                   | Per-release steps                                                                    |
| `docs/workflows/lint-format.md`                         | Lint/format/commit policy                                                            |
| `docs/workflows/dependency-tracking.md`                 | Upstream dependency watch (exiv2, LibRaw, WebKitGTK…)                                |

## Repo hygiene

- Fixtures: `python3 tools/fixture_gen.py --count <N> --seed 42 --out <dir>` (deterministic; HEIC copied from `tools/fixture-samples/`; generated data is never committed). CI smoke = 1000, decision runs = 50000.
- After merged PRs, keep backlog checkboxes in sync (see the `backlog-maintenance` skill).
- Commits: conventional commits (`feat:`, `fix:`) with the task ID in the footer, e.g. `Task: 01-014`.
