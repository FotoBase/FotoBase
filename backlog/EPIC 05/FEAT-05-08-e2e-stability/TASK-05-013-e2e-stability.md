# task: stabilize the e2e suite (edit-view hangs, tray quit race)

**Feature:** [FEAT-05-08-e2e-stability](FEAT-05-08-e2e-stability.md) · **Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** todo

## Description
The e2e suite (WebdriverIO + tauri-driver, PLAN §4 "Testing & tooling", `.github/workflows/e2e.yml`) is too flaky to gate merges: on 2026-11-03 it failed on `main` twice (runs 41616425604, 41638071493) and on PR #249 twice (run 41645698226 + its `--failed` rerun), always in the same two spec files, while the same specs' siblings passed in interleaved runs (41603548611, 41631337409, 41641652081). A suite that fails 2-of-4 runs trains the team to ignore red, which is worse than no gate.

## Functional specs
- `edit-view.e2e.ts` passes deterministically (3 consecutive green CI runs).
- `tray.e2e.ts` "quit fully exits the process" passes deterministically (3 consecutive green CI runs).
- A flaky spec failure is diagnosable from the CI log alone: the retry runs in a fresh session and the log shows which attempt failed and why.
- No test is deleted or weakened to achieve stability (the assertions below must keep covering their TASK specs).

## Technical specs
Known failure signatures (from the four runs above):
1. `edit view — undo reverts the recipe` (and siblings `applies an op from each tab…`, `Escape closes…`, `AfterTest Hook`): `WebDriverError: element not interactable` on a `/click` POST, then `Request timed out! Consider increasing the "connectionRetryTimeout"` on a later click, then the 180 s mocha timeout. The sidecar dump the spec prints (`SIDECAR AFTER BW:`) shows the recipe *was* applied — the edit pipeline works; the driver interaction is what hangs. Suspects, in order: (a) WebKitGTK under xvfb + software GL (libEGL "DRI3 error" in every run) intermittently stops hit-testing/focus after a preview re-render (the recipe preview repaint is the step right before the failing click); (b) the `activate()` helper ([edit-view.e2e.ts:93](../../../e2e/specs/edit-view.e2e.ts)) falls back from a WebDriver click to a DOM `el.click()` — if the webview is wedged, *both* fail and the waitUntil burns 2 s per op until the mocha timeout, so the real error is masked.
2. `tray — quit fully exits the process` ([tray.e2e.ts:111](../../../e2e/specs/tray.e2e.ts)): `fotobase process still alive after tray quit` — `quit_from_tray` is fire-and-forget over `window.__TAURI__` and the assertion polls `appProcesses()` for 10 s; a slow job-queue drain (§3 rule 4: exit drains the queues first) on a busy CI runner can exceed the window. Suspect: the 10 s `waitUntil` is racing an unbounded drain, not an actual regression.
3. `Failed to create a session` (once, `#0-3`): stale-driver/port race the wdio.conf port-wait logic ([wdio.conf.ts](../../../e2e/wdio.conf.ts)) was built to prevent — one more way through it exists.

Approach (work test-first even here: a repro is the test):
- Reproduce locally under `xvfb-run -a npm test` (see `e2e/README.md`), N iterations until the failure rate is measured; record the rate before and after each fix in the PR.
- For (1): first make the failure legible — `activate()` should surface *which* phase failed (native click vs DOM fallback vs verify) instead of timing out silently; then fix the root cause (candidates: force `document.activeElement` reset / blur between ops, disable smooth scroll effects under test, or move the flaky op dispatch to the same `browser.execute` path that already works elsewhere in the spec).
- For (2): measure the actual drain time; either extend the window to a measured bound with a comment, or make `quit_from_tray` answer-then-exit so the test can await an ack instead of polling the process table.
- For (3): keep the existing per-session driver/port discipline; harden the `waitPortsFree` loop (e.g. re-check after driver spawn, kill-orphan detection in the log on failure).
- Whatever the fix, `specFileRetries: 1` stays: retries are the bounded backstop, not the fix.

## Testing criteria (TDD)
- Local repro loop: `cd e2e && xvfb-run -a npm test` at least 5× before the fix, recording which specs fail; after the fix, 5 consecutive green runs (the skill's timing-sensitive rule: a test that fails 1-in-5 locally will fail in CI).
- CI: 3 consecutive green `e2e.yml` runs on this branch (re-run manually if needed) before calling it done.
- The fix may not touch app behavior: `cargo test --workspace` and the app Vitest suite stay green; if any app-side change is needed (e.g. `quit_from_tray` ack), it gets its own test.

## Acceptance criteria
- [ ] 05-013: `edit-view.e2e.ts` and `tray.e2e.ts` pass 3 consecutive CI runs; failure rate measured before/after in the PR; no assertion weakened.
