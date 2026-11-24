#!/usr/bin/env bash
# TASK-09-001 smoke: install the simulator build on a booted iPhone simulator,
# launch it, and require the process to stay alive. Run by the `mobile`
# workflow on macOS runners; also usable locally with Xcode installed.
set -euo pipefail

APP_ID="${APP_ID:-io.github.fotobase.app}"
SETTLE_SECONDS="${SETTLE_SECONDS:-30}"

app=$(find app/src-tauri/gen/apple/build -name '*.app' -path '*sim*' -maxdepth 6 | head -n 1)
[ -n "$app" ] || { echo "no simulator .app found" >&2; exit 1; }

udid=$(xcrun simctl list devices available -j |
  python3 -c 'import json,sys; d=json.load(sys.stdin)["devices"]; print(next(x["udid"] for k,v in d.items() if "iOS" in k for x in v if "iPhone" in x["name"]))')
xcrun simctl boot "$udid" || true
xcrun simctl bootstatus "$udid" -b
xcrun simctl install "$udid" "$app"
xcrun simctl launch "$udid" "$APP_ID"
sleep "$SETTLE_SECONDS"

if ! xcrun simctl spawn "$udid" launchctl list | grep -q "$APP_ID"; then
  echo "app is not running after launch" >&2
  exit 1
fi
echo "ios smoke OK: $APP_ID running after ${SETTLE_SECONDS}s"
