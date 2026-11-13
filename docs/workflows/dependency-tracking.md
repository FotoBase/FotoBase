# Upstream dependency tracking

**Governs:** review cadence for pinned/risky upstreams · **Plan ref:** §10 (risks & mitigations) · **Related:** `docs/workflows/release-checklist.md` (step 1)

PLAN §10 names specific drift risks. This doc is the mechanism. Deliberately manual until EPIC-01 CI exists; the `cargo audit` step folds into CI once the workspace does.

## Watch list

| Dependency            | Risk (§10)             | Mitigation in place                                                                                   | Watch for                                                         |
| --------------------- | ---------------------- | ----------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| exiv2                 | project health         | own bindings; metadata I/O behind a crate-internal trait; `kamadak-exif` read-only fallback           | new releases, security advisories, API breaks                     |
| LibRaw                | raw decode drift       | embedded-JPEG-preview fallback                                                                        | new camera support, security releases                             |
| libheif               | HEIC decode            | CI uses committed tiny samples, no libheif toolchain in CI (ADR-0009)                                 | security releases, HEIF spec changes                              |
| Tauri / wry (WebView) | OS WebView drift       | perf smoke per release; canvas fallback covers worst case; WebView triage pinned in release checklist | security advisories, breaking updates                             |
| WebKitGTK             | Linux perf regressions | webkitgtk perf protocol (00-012); DOM-vs-canvas decision (01-004)                                     | Ubuntu 24.04+ point releases moving the perf bars (floor per ADR-0014) |
| ffmpeg                | patents/licensing      | LGPL build, no non-free codecs, OS decoders where possible                                            | license changes, codec removals                                   |
| ort / ONNX models     | model licensing        | per-model license manifest checked in CI; permissively licensed only                                  | upstream license changes (InsightFace-style non-commercial traps) |
| sqlite-vec / rusqlite | catalog layer          | rebuildable cache by design (§3)                                                                      | breaking schema/API changes                                       |
| ort (ONNX runtime)    | pre-release pin        | pinned to 2.0.0-rc.x; no stable 2.0 exists yet                                                        | 2.0 stable release → bump in same PR as a `cargo test` gate        |
| libayatana-appindicator | deprecation          | system-tray integration (03-002)                                                                      | deprecation/removal from Ubuntu 24.04+ repos, replacement API      |

## Cadence

- **Per release** (release checklist step 1): `cargo audit` (once CI exists), `cargo outdated` / `npm outdated` review, and a manual pass over the watch list for security advisories.
- **Monthly or per-release, whichever first:** check new releases of exiv2, LibRaw, libheif, Tauri/wry, WebKitGTK (Ubuntu package tracker).
- Record anything action-worthy: security fixes → patch release per ADR-0012; API breaks → a backlog task; licensing changes → ADR (a licensing change cannot be absorbed silently).

## Rule of thumb

Bumping a pinned upstream is a normal task PR (`chore:`/`fix:`) with tests; changing _which_ upstream we use (e.g. swapping the exiv2 bindings backend) is an ADR.
