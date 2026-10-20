# task: build folder tree with selection and context menu

**Feature:** [FEAT-01-06-ui-library](FEAT-01-06-ui-library.md) · **Epic:** [EPIC-01 — Viewer & library MVP](../EPIC-01.md) · **Status:** todo

## Description
Implement the library folder tree per PLAN §2B ("Folders view: real on-disk folder tree, grouped by year") with working selection state and a context-menu skeleton that P2 commands will fill in.

## Functional specs
- Tree mirrors real disk folders, grouped by year; selecting a folder filters the grid.
- Selection state survives grid navigation (multi-select folders allowed).
- Context menu skeleton with P1 items wired (View slideshow, Refresh thumbnails, Locate on disk, Remove from Fotobase per §2) and placeholders for P2 entries.

## Technical specs
- Tree data from the `catalog` `folders` table (01-005), grouped by year from folder/file dates; Svelte 5 component.
- Folder selection emits the filter used by the grid; context menu reuses the 01-021 commands where they overlap.

## Testing criteria (TDD)
- Unit: fixture folders group under the correct year nodes.
- E2E: click a folder ⇒ grid shows only its photos; context menu opens with the P1 commands enabled.

## Acceptance criteria
- [ ] 01-015: Folder tree grouped by year, selection filters the grid, context-menu skeleton with P1 commands.
