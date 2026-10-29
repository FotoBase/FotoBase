# e2e (WebdriverIO + tauri-driver)

Drives the real Tauri app through WebKitWebDriver (PLAN §4). One spec so far:
`specs/stacking.e2e.ts` (TASK-03-021) — suggest and stack a RAW+JPEG pair, badge
and picker, switch the top, unstack, originals byte-identical.

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
```

## How it works

- `wdio.conf.ts` starts `tauri-driver` per session with a fresh temp `HOME`:
  empty catalog, empty thumbnail cache, default settings.
- `support/library.ts` makes a throw-away library (a RAW+JPEG pair and a solo
  photo) and records the originals' checksums.
- Specs hand the library to the app with the `set_folder_mode` IPC command
  (`window.__TAURI__`, same as the Folder Manager) — no app-side test hook.
- Selectors are the app's `data-testid`s.
