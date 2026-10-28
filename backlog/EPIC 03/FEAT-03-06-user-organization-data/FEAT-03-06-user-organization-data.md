# feat: user organization data

**Epic:** [EPIC-03 — Organize](../EPIC-03.md) · **Status:** planned

## Acceptance criteria

- Star (and opt-in 1–5 rating) writes `xmp:Rating` and survives catalog rebuild; starred filter works.
- Tags land in `dc:subject` (+ `lr:hierarchicalSubject`); Quick Tags panel applies one of 10 tags in a single click; captions land in `dc:description` and are searchable.
- Albums are create/delete/add/remove only — originals never touched; album membership rebuildable from `fb:Albums` sidecar data.
- Hidden folders/photos disappear from the library only; files on disk untouched.

- [x] `03-005` (M) Star/rating (per 00-001 decision) → `xmp:Rating`; starred filter.
- [x] `03-006` (M) Tags/keywords → `dc:subject` (+ `lr:hierarchicalSubject`); tag editor; Quick Tags panel (10 one-click buttons).
- [x] `03-007` (M) Captions → `dc:description`; searchable.
- [ ] `03-008` (M) Albums: create (name/date/place/description), delete, add/remove photos; `fb:Albums` namespace; album list UI.
- [ ] `03-009` (S) Hidden folders + hidden photos (library only, files untouched).

## Tasks

- [x] [TASK-03-005-star-rating.md](TASK-03-005-star-rating.md) — star/rating written to xmp:Rating with starred filter
- [x] [TASK-03-006-tags-quick-tags.md](TASK-03-006-tags-quick-tags.md) — tags, tag editor, and quick tags panel
- [x] [TASK-03-007-captions.md](TASK-03-007-captions.md) — captions written to dc:description
- [ ] [TASK-03-008-albums.md](TASK-03-008-albums.md) — albums with fb:Albums sidecar namespace
- [ ] [TASK-03-009-hidden-folders-photos.md](TASK-03-009-hidden-folders-photos.md) — hidden folders and hidden photos
