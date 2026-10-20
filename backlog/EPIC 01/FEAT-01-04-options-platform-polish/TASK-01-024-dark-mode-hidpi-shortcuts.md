# task: polish dark mode, hidpi, and keyboard shortcuts

**Feature:** [FEAT-01-04-options-platform-polish](FEAT-01-04-options-platform-polish.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** todo

## Description
Deliver the platform-polish P1 items from PLAN §2 additions ("Dark mode / HiDPI: native theming, sharp on 4K/Retina") and §2M ("Keyboard shortcuts: full keyboard control"): dark mode and HiDPI correctness on all three OSes, plus documented shortcuts for every core P1 flow.

## Functional specs
- Dark mode follows the OS setting (or app toggle) on Windows, macOS, Linux; all P1 screens have dark variants.
- Thumbnails and UI are sharp on 4K/Retina displays on all three OSes.
- Keyboard shortcuts cover core flows: navigate grid/viewer, open/close viewer, slideshow, search focus, exit.

## Technical specs
- Plain CSS / CSS variables theming (§4) with a `prefers-color-scheme` hook; HiDPI via device-pixel-ratio-aware thumbnails from the 01-010 pipeline.
- Shortcut list documented in docs/help; shortcuts as Svelte actions, testable.

## Testing criteria (TDD)
- E2E: toggle OS theme ⇒ app swaps CSS variables (snapshot test on both themes).
- E2E: each documented shortcut triggers its action (grid navigation, viewer open/close, search focus).

## Acceptance criteria
- [ ] 01-024: Dark mode + HiDPI correct on all three OSes; core-flow keyboard shortcuts implemented and documented.
