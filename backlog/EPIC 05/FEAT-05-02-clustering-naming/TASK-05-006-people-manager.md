# task: build people manager dialog

**Feature:** [FEAT-05-02-clustering-naming](FEAT-05-02-clustering-naming.md) · **Epic:** [EPIC-05 — People](../EPIC-05.md) · **Status:** todo

## Description
People Manager dialog listing all people — named and unnamed clusters — with representative faces (PLAN §2 Tools menu "People Manager"; §6 P3 People).

## Functional specs
- Tools → People Manager opens a dialog listing every named person and every unnamed cluster.
- Each entry shows representative face thumbnails and photo counts.
- Renaming a person updates all their faces; entry points to the unnamed queue for unnamed clusters.

## Technical specs
- Svelte dialog over Tauri commands; data from `people` + `faces` catalog tables.
- Rename = update person + rewrite MWG region names on all confirmed faces (sidecar writes).

## Testing criteria (TDD)
- E2e: open People Manager, verify named + unnamed entries with counts.
- Integration: rename a person → sidecar regions updated; catalog rebuild preserves new name.

## Acceptance criteria
- [ ] 05-006: People Manager lists all named people and unnamed clusters with thumbnails/counts.
