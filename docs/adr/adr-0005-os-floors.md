# ADR-0005: Minimum OS versions

**Date:** 2026-10-20 · **Status:** accepted (PLAN §11 Q4)

## Context

The positioning is "runs on old hardware", but WebView support sets a floor.

## Decision

Windows 10 22H2+ (WebView2 preinstalled), macOS 12+ Intel + Apple Silicon (WKWebView 12+), Ubuntu 22.04+ x64/arm64 (WebKitGTK 2.40+). CI matrix uses these floors.

## Consequences

Large install base; WebKitGTK remains the weakest WebView — benchmarked per docs/webkitgtk-perf-protocol.md with the canvas-grid fallback.
