# e2e (WebdriverIO + tauri-driver)

Drives the real Tauri app through WebKitWebDriver (PLAN §4). Specs:
`specs/stacking.e2e.ts` (TASK-03-021) — suggest and stack a RAW+JPEG pair, badge
and picker, switch the top, unstack, originals byte-identical;
`specs/tray.e2e.ts` (TASK-03-025) — close the window and watching continues,
a file dropped while hidden still lands in the catalog, quit fully exits.

`specs/map-basemap.e2e.ts` (TASK-06-001) — the map panel opens on the
bundled offline PMTiles basemap, pans and zooms, and the instrumented
fetch/XHR stack records zero http(s) requests.

## Platforms

`tauri-driver` has no macOS support, so the suite runs on Linux (CI, or a Linux
container) and could run on Windows with `msedgedriver` (not wired up). CI:
`.github/workflows/e2e.yml` (push to `main`, manual, weekly, and PRs touching
`e2e/`).

## Run locally (Linux)

```
cargo install tauri-driver --locked          # plus webkit2gtk-driver and xvfb
(cd app && npm ci && npm run build)
cargo build -p fotobase --features tauri/custom-protocol
cd e2e && npm ci
xvfb-run -a npm test                         # FOTOBASE_E2E_APP overrides the binary path
npm run test:unit                            # harness helpers only (node:test; runs on macOS too)
```

## How it works

- `wdio.conf.ts` starts `tauri-driver` per session with a fresh temp `HOME`:
  empty catalog, empty thumbnail cache, default settings. Before and after
  every session it reaps leftover drivers and app instances
  (`support/procs.ts`, shell-free: `pkill -f` inside `execSync` matches and
  kills its own shell on Linux) and logs what it had to kill as
  `[e2e] … reaped …` — the first thing to check when a spec fails.
- `support/library.ts` makes a throw-away library (a RAW+JPEG pair and a solo
  photo) and records the originals' checksums.
- `support/interact.ts` `activate()` clicks overlay controls with ONE DOM
  dispatch and then waits for the effect. WebKitWebDriver's native click on
  those controls hangs or reports "not interactable" under xvfb, and a second
  "fallback" click would toggle the control back. Errors name the phase
  (`find` / `dispatch` / `verify`).
- Specs hand the library to the app with the `set_folder_mode` IPC command
  (`window.__TAURI__`, same as the Folder Manager) — no app-side test hook.
- Selectors are the app's `data-testid`s.
