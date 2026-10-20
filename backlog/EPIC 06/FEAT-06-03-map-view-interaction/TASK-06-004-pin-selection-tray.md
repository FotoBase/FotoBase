# task: implement pin click and selection interactions

**Feature:** [FEAT-06-03-map-view-interaction](FEAT-06-03-map-view-interaction.md) · **Epic:** [EPIC-06 — Places](../EPIC-06.md) · **Status:** todo

## Description
Map interactions: clicking a pin opens the photo in the viewer; selecting pins feeds the Photo Tray (PLAN §2D Map view; §6 P3 Places "clicking a pin opens the viewer, selecting pins feeds the Photo Tray").

## Functional specs
- Click a pin → open that photo in the viewer.
- Multi-select pins (e.g. by clicking several, or cluster selection) → selected photos load into the Photo Tray for batch actions.
- Interactions work with touch and mouse.

## Technical specs
- Pin → photo id mapping from the map data source; viewer open via existing navigation command.
- Photo Tray integration from EPIC-03: selected ids appended to tray (respecting Hold pins).

## Testing criteria (TDD)
- E2e: click a pin → viewer opens with that photo.
- E2e: select multiple pins → tray contains exactly those photos; a tray action (e.g. export) applies to them.
- E2e: touch gestures select/open on a touch-enabled profile.

## Acceptance criteria
- [ ] 06-004: Pin click opens the viewer; pin selection feeds the Photo Tray; touch + mouse supported.
