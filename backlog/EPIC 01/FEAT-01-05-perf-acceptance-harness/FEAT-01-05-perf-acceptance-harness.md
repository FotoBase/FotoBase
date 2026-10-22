# feat: perf acceptance harness

**Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** planned

## Acceptance criteria

- Harness measures time-to-first-thumb, idle/peak RAM, and scroll fps on the 00-013 fixture per OS, emitting machine-readable results.
- Runs as a CI smoke test and fails loudly when the §6 P1 bars are missed (< 2 s first thumbs, < 150 MB idle RAM, 60 fps scroll).
- Historical results are comparable across runs (same fixture seed, same metrics, stable environment notes).

- [x] `01-026` (M) Perf harness on 00-013 fixture: time-to-first-thumb, idle/peak RAM, scroll fps per OS; runs in CI as smoke test (per §10 WebView-drift risk).

## Tasks

- [ ] [TASK-01-026-perf-harness.md](TASK-01-026-perf-harness.md) — build perf acceptance harness
