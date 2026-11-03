# ADR-0014: Linux floor moves to Ubuntu 24.04+

**Date:** 2026-11-03 · **Status:** accepted (supersedes the Ubuntu part of [ADR-0005](adr-0005-os-floors.md))

## Context

The `ai` crate (TASK-05-001) links ONNX Runtime via `ort` 2.0.0-rc.15 `download-binaries`. The only prebuilt Linux x86_64/arm64 distribution pyke serves (ONNX Runtime 1.31.0, `cdn.pyke.io/0/pyke:ort-rs/ms@1.31.0`) is built on an Ubuntu 24.04-era toolchain and requires:

- **glibc ≥ 2.38** — `__isoc23_strtol`, `__isoc23_strtoll`, `__isoc23_strtoull` (referenced from ONNX Runtime's absl/nlohmann objects),
- **libstdc++ from GCC ≥ 13** — `std::__cxx11::basic_string::_M_replace_cold` (char and wchar_t instantiations).

Ubuntu 22.04 ships glibc 2.35 and GCC 11, so any binary statically linking these prebuilts fails to link on 22.04 — and would fail at load time for 22.04 users even if built elsewhere. There is no alternative feature set in `ort-sys`'s `dist.tsv`; one dist per target.

Alternatives considered and rejected:

- **Build ONNX Runtime from source on 22.04** — adds 20–40 min to every cold CI/release build and a large build dependency surface, for one distro.
- **Bundle Microsoft's official `libonnxruntime.so`** — changes the packaging model (dynamic library shipping, rpath handling) purely to keep one older release.
- **glibc/libstdc++ shims + PPA-provided gcc-13 on the build host** — fragile toolchain surgery in CI and release; jammy has no gcc-13 in the archive.

## Decision

The Linux floor rises from Ubuntu 22.04+ to **Ubuntu 24.04+** (x64, arm64). Windows 10 22H2+ and macOS 12+ floors (ADR-0005) are unchanged.

- CI matrix and release builds use `ubuntu-24.04` as the Linux leg.
- The WebKitGTK floor moves with the distro: Ubuntu 24.04 ships WebKitGTK 2.44.x, so the effective WebView floor on Linux becomes 2.44+ (was 2.40).
- The perf decision image (`perf/Dockerfile.decision`) and the perf protocol reference machine track the new floor.

## Consequences

- The "runs on old hardware" positioning narrows on Linux: Ubuntu 22.04 users (EOL standard support April 2027) are no longer supported. Windows and macOS positioning is unaffected.
- `ort` prebuilt binaries link and run as-is; no shim or source build needed.
- WebKitGTK perf bars are re-benchmarked against the 24.04-era WebKitGTK per docs/webkitgtk-perf-protocol.md; prior decision runs recorded in ADR-0013 were captured on a 22.04-based image and are treated as historical.
