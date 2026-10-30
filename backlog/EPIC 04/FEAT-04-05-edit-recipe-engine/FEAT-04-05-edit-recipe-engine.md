# feat: edit recipe engine

**Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** planned

## Acceptance criteria

- Recipe ops are pure functions over linear-light buffers; identical op lists produce pixel-identical results at preview and export resolution (golden-image tests, per-pixel tolerance).
- Recipes validate against the 00-011 JSON Schema; older recipe versions migrate forward.
- Undo/redo/undo-all/revert work with per-photo history; recipes round-trip through `fb:Recipe` XMP and survive catalog rebuild.

- [x] `04-001` (L) Recipe engine core: versioned ops model (00-011 spec), pure functions over linear-light buffers, same code for preview (screen-res) and export (full-res).
- [x] `04-002` (M) Recipe stored in `fb:Recipe` XMP namespace; versioning + migration path.
- [ ] `04-003` (M) Undo / redo / undo all / revert; per-photo edit history.
- [ ] `04-004` (M) Golden-image test harness per recipe op (per-pixel tolerance, §9).

## Tasks

- [ ] [TASK-04-001-recipe-engine-core.md](TASK-04-001-recipe-engine-core.md) — build recipe engine core
- [ ] [TASK-04-002-recipe-xmp-storage.md](TASK-04-002-recipe-xmp-storage.md) — store recipes in fb:recipe xmp
- [ ] [TASK-04-003-undo-redo-history.md](TASK-04-003-undo-redo-history.md) — implement undo, redo, and per-photo edit history
- [ ] [TASK-04-004-golden-image-harness.md](TASK-04-004-golden-image-harness.md) — build golden-image test harness for recipe ops
