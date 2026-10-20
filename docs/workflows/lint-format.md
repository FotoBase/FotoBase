# Lint, format & commit policy

**Governs:** all code changes · **Enforced by:** CI (TASK-01-002) once the scaffold exists · **Plan ref:** §4

## Tools

| Layer                | Check                                                   | Config                                     |
| -------------------- | ------------------------------------------------------- | ------------------------------------------ |
| Rust (all crates)    | `cargo fmt --check`                                     | `rustfmt.toml` at workspace root           |
| Rust (all crates)    | `cargo clippy --workspace --all-targets -- -D warnings` | lints in workspace `Cargo.toml`            |
| Frontend (`app/src`) | eslint + prettier                                       | `eslint.config.*`, `.prettierrc` in `app/` |
| Frontend             | `svelte-check` type check                               | part of the Vite build                     |

Config files land with the EPIC-01 scaffold (TASK-01-001/002) and CI runs the table above on every PR. Red CI blocks merge.

## Policy

- Format before committing; a PR that only reformats unrelated files is noise — run `fmt` on the files you touched.
- Clippy warnings are errors in CI; fix or `#[allow]` with a comment explaining why.
- New public Rust APIs: run `cargo doc` locally; doc warnings fail the release build check.

## Commit style

- Conventional commits: `feat:`, `fix:`, `docs:`, `test:`, `chore:`, `refactor:`.
- Task ID in the footer: `Task: 01-014`. One task ≈ one PR (CONTRIBUTING.md).
- Branches: `feat/<task-id>-slug`, `fix/<task-id>-slug`.

## Docs

- Markdown changes (backlog, PLAN, ADRs, skills) follow the existing file conventions: ADRs use Date · Status · Context · Decision · Consequences; TASK/FEAT/epic checkbox lines mirror 1:1 (see the `backlog-maintenance` skill).
