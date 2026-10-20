# task: add on-this-day memories view

**Feature:** [FEAT-07-01-ai-assists](FEAT-07-01-ai-assists.md) · **Epic:** [EPIC-07 — Smart search](../EPIC-07.md) · **Status:** todo

## Description
A "Memories" view that resurfaces photos taken on today's date in past years (PLAN §2 2026 additions: "Memories ('On this day'): resurface photos taken on this date in past years"). Pure catalog query — no AI model required.

## Functional specs
- Opening Memories shows photos taken on this calendar day in previous years, grouped by year, newest year first.
- Empty years are skipped; if there are no memories at all, the view is reachable but shows a friendly empty state.
- Tapping a memory photo opens the normal viewer; the view respects existing filters/hidden photos.
- The view never modifies data — read-only browsing.

## Technical specs
- `catalog` query: `taken_at` month/day match, grouped by year; excludes hidden photos; straightforward SQL over `files`/`metadata`.
- UI (Svelte): one new view route reusing the existing virtualized grid components; no new rendering machinery.
- No sidecar writes, no derived data — a live query, nothing to rebuild.

## Testing criteria (TDD)
- Integration: fixture with photos on 2023-10-20, 2024-10-20, 2024-10-21 → memories for "today = 2024-10-20" show the 2023 and 2024-10-20 photos grouped by year, not the 10-21 one.
- E2E: Memories view renders and a photo click opens the viewer.

## Acceptance criteria
- [ ] 07-008: Memories view resurfaces "on this day" content grouped by year, read-only.
- [ ] 07-008: Hidden photos and empty years are excluded with a clean empty state.
