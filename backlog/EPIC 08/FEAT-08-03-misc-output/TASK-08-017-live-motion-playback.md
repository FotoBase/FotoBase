# task: add live and motion photo playback

**Feature:** [FEAT-08-03-misc-output](FEAT-08-03-misc-output.md) · **Epic:** [EPIC-08 — Output & sharing](../EPIC-08.md) · **Status:** todo

## Description
Play Apple Live Photos and Android Motion Photos (PLAN §2 2026 additions, P4 phasing). A still+video pair plays its embedded video on press/hold in the viewer.

## Functional specs
- Live/Motion photos show a badge in the grid/viewer; pressing (or clicking a play button) plays the embedded clip, release returns to the still.
- Detection is automatic at scan time: Apple's Live Photo (still + `Core Media`-tagged MOV sidecar or embedded) and Android Motion Photo (video embedded in JPEG trailer).
- Non-Live photos are unaffected; unsupported variants degrade to plain still+video.

## Technical specs
- Detection in `scanner`/`video` crates: Android Motion Photo = JPEG + MP4 in trailer (Google's XMP `GCamera:MicroVideo`/ContainerDirectory markers); Apple = JPEG/HEIC + matching MOV (`ContentIdentifier` EXIF/XMP pairing) in the same folder.
- Playback: extract the clip to thumbnail-cache temp (ffmpeg demux for the embedded-trailer case) and feed the existing HTML5/`stream://` video path from EPIC-02's video playback work (PLAN §2 Video strategy).
- Catalog: media_type/subtype recorded at scan so the badge needs no per-view probing.

## Testing criteria (TDD)
- Integration: fixture Motion Photo JPEG → scanner detects clip, viewer plays it.
- Integration: fixture Apple pair (HEIC + MOV with matching ContentIdentifier) → detected and played.
- Unit: plain JPEG with appended garbage is NOT detected as Motion Photo.

## Acceptance criteria
- [ ] 08-017: Live/Motion photos (Apple + Android variants) play back via press-and-hold in the viewer.
- [ ] 08-017: Detection happens at scan time; non-Live media is unaffected.
