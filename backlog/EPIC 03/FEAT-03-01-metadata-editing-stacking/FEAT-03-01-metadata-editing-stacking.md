# feat: metadata editing & stacking

**Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** planned

## Acceptance criteria

- Capture-time shift, timezone fix, and multi-camera merge produce consistent ordering and write to sidecars; rebuildable.
- Info-pane editing of date, GPS, artist, copyright round-trips through XMP.
- RAW+JPEG pairs, bursts, and edited versions stack under one thumbnail with a working stack picker; stacking is metadata-only.

- [x] `03-019` (M) Date/time adjustment: shift capture time, fix timezones, merge multi-camera sets.
- [ ] `03-020` (M) Metadata editing in info pane: date, GPS, artist, copyright.
- [ ] `03-021` (M) Stacking: RAW+JPEG pairs, burst stacks, edited versions under one thumbnail.

## Tasks

- [x] [TASK-03-019-datetime-adjustment.md](TASK-03-019-datetime-adjustment.md) — date/time adjustment: shift, timezone fix, multi-camera merge
- [ ] [TASK-03-020-info-pane-metadata-editing.md](TASK-03-020-info-pane-metadata-editing.md) — info-pane metadata editing: date, GPS, artist, copyright
- [ ] [TASK-03-021-stacking.md](TASK-03-021-stacking.md) — stacking: RAW+JPEG pairs, bursts, edited versions
