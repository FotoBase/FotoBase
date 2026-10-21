# task: build options dialog (general tab)

**Feature:** [FEAT-01-04-options-platform-polish](FEAT-01-04-options-platform-polish.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** in progress (2026-10-21)

## Description
Implement the Options dialog General tab per PLAN §2 Options inventory / §6 P1 deliverables: language, automatic updates (auto / check-but-ask / off), clear cache files, show tooltips. All settings persist.

## Functional specs
- Language selection changes UI language (i18n scaffold at minimum; full translations can follow).
- Update modes behave as labeled: auto installs, check-but-ask prompts, off never phones home (§1: no telemetry beyond update checks).
- "Clear cache files" empties the thumbnail cache safely — running renders finish or cancel cleanly; the library itself is untouched.
- Show tooltips toggles hover tooltips.

## Technical specs
- Settings persisted (Tauri store or a small settings table); updater behavior drives the signed Tauri updater plugin (01-003).
- Cache clear calls into the 01-010 cache with the job queue paused/drained for affected keys.

## Testing criteria (TDD)
- Unit: each setting round-trips through persistence.
- Integration: clear-cache while a render job is in flight ⇒ cache ends empty, no partial files, no crash.

## Acceptance criteria
- [x] 01-022: Options dialog (General) with language, update modes, clear cache, tooltips — all persisted.
