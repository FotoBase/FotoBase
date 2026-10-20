# task: set minimum OS version floors

**Feature:** [FEAT-00-01-decisions](FEAT-00-01-decisions.md) · **Epic:** [EPIC-00 — Foundation & decisions](../EPIC-00.md) · **Status:** done

## Description
Resolve PLAN §11 Q4: supported OS floors. Decided 2026-10-20: Windows 10 22H2+, macOS 12+ (Intel + Apple Silicon), Ubuntu 22.04+ (x64, arm64). Matches the "runs on old hardware" positioning while keeping WebView support floors (WebView2 preinstalled, WKWebView 12+, WebKitGTK 2.40+).

## Functional specs
- Installer/build targets refuse or warn below the floors.
- Documented system requirements on the project site/docs.

## Technical specs
- CI matrix (task 01-002) builds against exactly these floors.
- Tauri bundler targets: MSI/NSIS (Win10 22H2+), DMG (macOS 12+), AppImage/deb/rpm (Ubuntu 22.04+).

## Testing criteria (TDD)
- CI: build + test jobs run on windows-latest, macos-latest (arm64 + x64), ubuntu-latest and stay green.

## Acceptance criteria
- [x] 00-003: Floors recorded in PLAN §1 + §11 Q4 (Win 10 22H2+, macOS 12+, Ubuntu 22.04+).
- [x] CI matrix (01-002) configured to use these floors.
