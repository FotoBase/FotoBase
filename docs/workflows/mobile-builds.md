# Mobile builds (iOS / Android)

**Governs:** Tauri v2 mobile targets and the `mobile` CI workflow · **Plan ref:** §6 Phase 5 · **Task:** TASK-09-001

One Rust core, one frontend. The `fotobase` app crate (`app/src-tauri`) is the same `staticlib`/`cdylib` on every platform; `#[cfg_attr(mobile, tauri::mobile_entry_point)]` on `run()` is the mobile entry.

## What differs from desktop (minimal cfg gates)

| Area | Desktop | Mobile |
| ---- | ------- | ------ |
| HEIC/AVIF (libheif), RAW (LibRaw) | linked (macOS/Linux) | off (`fb_native_codecs` cfg in `crates/render/build.rs`); the dispatch reports them unsupported |
| Updater, global shortcut (screen capture), tray / close-to-tray, single-instance | on | off; stores update mobile apps. `check_for_updates` reports "not checked" |
| Delete from disk, Locate on disk | OS trash / file manager | refuse with an error — never a permanent delete (PLAN §3 rule 1) |
| File associations, context menu | registered at runtime | declared in the app manifest instead |
| Capabilities | `capabilities/default.json` | `capabilities/mobile.json` (no `updater:*`) |

Desktop-only crates (`trash`, `tauri-plugin-updater`, `tauri-plugin-global-shortcut`) sit in a `cfg(not(any(android, ios)))` dependency table in `app/src-tauri/Cargo.toml`. Use `cfg(desktop)` / `cfg(mobile)` (set by `tauri-build`) in app code.

## CI lanes

`.github/workflows/mobile.yml` runs on every PR and on `main`:

- **android** (ubuntu-24.04): `tauri android build --debug --apk` for aarch64 + x86_64, then `tools/mobile_smoke_android.sh` on an x86_64 emulator (API 30).
- **ios** (macos-15): `tauri ios build --debug --target aarch64-sim`, then `tools/mobile_smoke_ios.sh` on an iPhone simulator.

Both lanes are `continue-on-error: true` while the toolchains settle. To promote a lane, delete that line and add the job to the required checks. The native projects (`app/src-tauri/gen/{android,apple}`) are generated per run and git-ignored.

Smoke = the app installs, launches, and the process is still alive after 30 s with no crash in the log.

## Local setup

- Android: JDK 17, Android SDK + NDK 27 (`ANDROID_HOME`, `NDK_HOME`), then `cd app && npx tauri android init && npx tauri android build --debug --apk`.
- iOS (macOS only): full Xcode, then `cd app && npx tauri ios init && npx tauri ios build --debug --target aarch64-sim`.
- Fast compile-only check of the core: `cargo check -p fotobase --lib --target aarch64-linux-android` (needs NDK clang on `PATH` or `CC_aarch64_linux_android`).

## Breaking-the-lane check

Verified once on PR #299 (an unresolved `tauri_plugin_updater` import turned both lanes red; reverted). A PR that breaks the mobile build must fail the mobile lane. To verify, push a throwaway commit that adds a desktop-only call (e.g. `trash::delete("x")`) outside `#[cfg(desktop)]` and confirm both lanes go red; drop the commit afterwards.
