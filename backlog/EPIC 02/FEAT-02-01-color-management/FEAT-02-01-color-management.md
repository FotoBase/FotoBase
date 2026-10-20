# feat: color management (P1.5 display scope)

**Epic:** [EPIC-02 — Media & platform hardening](../EPIC-02.md) · **Status:** planned

## Acceptance criteria

- Preview colors match the OS viewer (Preview.app, Photos) on a wide-gamut display on macOS and Windows; Linux falls back to sRGB without artifacts.
- Display-profile changes (monitor replug, profile switch) are picked up without restart.
- Golden-image tests for the display transform pass in CI on fixtures covering sRGB, Display-P3, and Adobe RGB sources.

- [ ] `02-001` (M) Read display ICC profile (macOS/Windows); color-managed preview path in render crate; sRGB fallback on Linux.
- [ ] `02-002` (S) Golden-image tests for display transform; visual comparison vs OS viewers.

## Tasks

- [ ] [TASK-02-001-display-icc-preview.md](TASK-02-001-display-icc-preview.md) — color-managed preview with display ICC profile
- [ ] [TASK-02-002-golden-image-tests.md](TASK-02-002-golden-image-tests.md) — golden-image tests for the display transform
