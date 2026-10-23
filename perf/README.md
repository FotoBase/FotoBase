# perf harness (TASK-01-026)

Implements `docs/webkitgtk-perf-protocol.md` (task 00-012) — that doc is
normative for what is measured and how.

## Layout

- `src/report.ts` — pure core: §2 bars, §3 `perf-report.json` shape, schema
  validation, and the task's meta self-check. Unit-tested (Vitest).
- `src/harness.ts` — the runner: WebDriver session (WebdriverIO +
  `tauri-driver`), in-page rAF fps/jank measurement, process-tree RSS
  sampling, report emission.

## Self-check (task testing criteria)

```
npm install
npx tsx src/harness.ts --self-check   # injects +5 s ⇒ miss, exit non-zero
npx vitest run                        # unit tests
```

## Full run

```
python3 tools/fixture_gen.py --count 50000 --seed 42 --out /tmp/fixture
(cd app && npm ci && npm run build)
cargo build -p fotobase --features tauri/custom-protocol
npx tsx src/harness.ts --fixture /tmp/fixture --count 50000 \
  --renderer dom --out perf/results/<os>-<date>.json
```

- Cold start is guaranteed by the harness pointing the app's data dir at a
  fresh temp HOME (empty catalog + empty thumbnail cache, protocol §1).
- The renderer under test is pinned through the app's settings store
  (`--renderer dom|canvas`).
- The app is armed through `FOTOBASE_PERF_FIXTURE` / `FOTOBASE_PERF_COUNT`
  (inert in every normal launch): it adds the fixture as a watched folder at
  startup and exposes the `perf_probe` timestamps command.
- Exit code: `0` all §2 bars pass, `1` a bar missed, `2` harness error.
- `--smoke` (CI, protocol §5): exit code only reflects production + schema
  validity of the report; bars are never enforced on CI hardware.

Decision runs (50k, reference hardware) are recorded per release under
`perf/results/` for trend comparison (protocol §4).
