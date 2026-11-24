#!/usr/bin/env bash
# TASK-09-001 smoke: install the debug APK on the running emulator, launch it,
# and require the process to stay alive without a crash. Run by the `mobile`
# workflow inside android-emulator-runner; also usable against a local device.
set -euo pipefail

APP_ID="${APP_ID:-io.github.fotobase.app}"
SETTLE_SECONDS="${SETTLE_SECONDS:-30}"

apk=$(find app/src-tauri/gen/android/app/build/outputs/apk -name '*.apk' -path '*debug*' | head -n 1)
[ -n "$apk" ] || { echo "no debug APK found" >&2; exit 1; }

adb wait-for-device
adb logcat -c
adb install -r "$apk"
adb shell monkey -p "$APP_ID" -c android.intent.category.LAUNCHER 1
sleep "$SETTLE_SECONDS"

if ! adb shell pidof "$APP_ID" >/dev/null; then
  echo "app is not running after launch" >&2
  adb logcat -d | tail -n 200
  exit 1
fi
if adb logcat -d | grep -E "FATAL EXCEPTION|Fatal signal" | grep -q .; then
  echo "app crashed during launch" >&2
  adb logcat -d | tail -n 200
  exit 1
fi
echo "android smoke OK: $APP_ID running after ${SETTLE_SECONDS}s"
