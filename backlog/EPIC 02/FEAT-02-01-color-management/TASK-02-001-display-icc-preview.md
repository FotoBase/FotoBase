# task: color-managed preview with display ICC profile

**Feature:** [FEAT-02-01-color-management](FEAT-02-01-color-management.md) · **Epic:** [EPIC-02 — Media & platform hardening](../EPIC-02.md) · **Status:** in review (2026-10-23)

## Description
Read the display ICC profile on macOS and Windows and run the preview render path through a color-managed transform in the `render` crate, so previews match OS viewers (PLAN §6 P1.5 acceptance). Linux falls back to sRGB. Per PLAN §2 D, previews are served via `preview://`/`thumb://` URI schemes from disk cache — the transform applies at render time.

## Functional specs
- Previews on a wide-gamut display match Preview.app / Photos on macOS and the Windows viewer.
- Monitor replug or profile switch is picked up without app restart.
- Linux (WebKitGTK) renders sRGB fallback with no artifacts (no double-transform).
- Source profiles: embedded ICC in files, plus sRGB, Display-P3, Adobe RGB fixtures.

## Technical specs
- `render` crate: add display-transform stage after decode/recipe render, before cache write or at serve time (pick one; cache must not be display-profile-dependent — key or invalidate on profile change).
- Profile acquisition: macOS `ColorSync`/`CGDisplayCopyColorSpace`-equivalent via `lcms2` + system API; Windows via `ICM2` (`WcsGetDefaultColorProfile`); read the profile into `lcms2` for transforms.
- Use `lcms2` (LittleCMS) for all transforms; never hand-roll color math.
- Profile-change notification per OS (display reconfiguration callbacks).

## Testing criteria (TDD)
- Unit: given an Adobe RGB fixture rendered with a Display-P3 display profile, output pixels differ from the sRGB-fallback path in the expected direction (wider saturation), within lcms2 tolerance.
- Integration: swapping the display profile mid-session invalidates/re-renders; old cached preview is not reused.
- E2e/manual: visual comparison vs OS viewers on macOS and Windows wide-gamut hardware (documented protocol, screenshots in PR).

## Acceptance criteria
- [x] 02-001: previews match the OS viewer on macOS and Windows wide-gamut displays.
- [x] 02-001: display-profile changes are picked up without restart.
- [x] 02-001: Linux uses sRGB fallback with no artifacts.
- [x] 02-001: transform runs in the `render` crate via lcms2; cached previews are profile-aware.
