# feat: additional decoders

**Epic:** [EPIC-04 — Edit & export](../EPIC-04.md) · **Status:** planned

## Acceptance criteria

- RAW files from each major camera vendor decode via LibRaw, or fall back to embedded JPEG preview for unsupported bodies — no hard failures.
- RAW + JPEG pair from each major vendor stacks and exports correctly (epic acceptance).
- JPEG XL decodes for thumbnails and full export; both decoders feed the same render/job pipelines without special cases.

- [ ] `04-013` (L) RAW decode via LibRaw bindings; embedded-JPEG-preview fallback for unsupported cameras.
- [ ] `04-014` (M) JPEG XL via jxl-oxide.

## Tasks

- [ ] [TASK-04-013-raw-libraw-decode.md](TASK-04-013-raw-libraw-decode.md) — implement raw decoding with libraw
- [ ] [TASK-04-014-jxl-decode.md](TASK-04-014-jxl-decode.md) — implement jpeg xl decoding
