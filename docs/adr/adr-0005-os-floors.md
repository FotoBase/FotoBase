# ADR-0005: Minimum OS versions

**Date:** 2026-10-20 · **Status:** accepted (PLAN §11 Q4)

## Context

The positioning is "runs on old hardware", but WebView support sets a floor.

## Decision

Windows 10 22H2+ (WebView2 preinstalled), macOS 12+ Intel + Apple Silicon (WKWebView 12+), Ubuntu 22.04+ x64/arm64 (WebKitGTK 2.40+). CI matrix uses these floors.

## CI coverage of the floors (2026-10-21)

- **Ubuntu:** floor (`ubuntu-22.04`) and current (`ubuntu-24.04`) legs — WebKitGTK is distro-pinned, so both ends need testing.
- **Windows:** one `windows-latest` leg; WebView2 is evergreen on user machines regardless of the build runner's OS.
- **macOS:** one arm64 leg + one native x64 leg on the oldest GA images. GitHub hosts no macOS 12/13 runners, so the **WKWebView 12 floor cannot be exercised in CI** — the runner's WKWebView is always newer. The floor is enforced at bundler level (TASK-01-003, `LSMinimumSystemVersion`/deployment target) and validated on-device before releases (release checklist).

## Consequences

Large install base; WebKitGTK remains the weakest WebView — benchmarked per docs/webkitgtk-perf-protocol.md with the canvas-grid fallback.
