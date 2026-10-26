# ffmpeg sidecar (TASK-02-003)

Fotobase ships an ffmpeg binary as a Tauri sidecar for video metadata,
thumbnails (TASK-02-004) and the `stream://` transcode fallback (TASK-02-006).
PLAN §10 risk row: *"Ship LGPL build without non-free codecs; use OS decoders
where possible."*

## What ships

- **ffmpeg** built from the pinned upstream source tarball
  (`build.sh`, version + SHA-256 pinned in the script) with an explicitly
  LGPL-only configuration: `--disable-gpl --disable-nonfree`, no external
  GPL/nonfree codec libraries. Encoders kept: `mpeg4`, `libvpx` (VP8),
  `libvpx-vp9` (VP9), `libopenh264` (BSD) — enough for the transcode fallback
  targets (H.264/mp4 or VP9/webm per PLAN §2).
- **Never shipped**: libx264/libx265 (GPL), hardware-SDK encoders
  (nvenc/qsv/amf/videotoolbox), anything enabled by `--enable-nonfree`.

Decoding of phone clips (H.264/HEVC) is primarily the OS WebView's job
(PLAN §2 Video strategy); the sidecar's decode needs are covered by the
built-in decoders of the LGPL build.

## License manifest

| Component | Version | License | Source |
| --------- | ------- | ------- | ------ |
| ffmpeg source | 7.1.1 | LGPL-2.1-or-later | <https://ffmpeg.org/releases/ffmpeg-7.1.1.tar.xz> (SHA-256 pinned in `build.sh`) |
| libvpx | bundled in ffmpeg | BSD-3-Clause | upstream ffmpeg tarball |
| libopenh264 | bundled in ffmpeg | BSD-2-Clause | upstream ffmpeg tarball |

The binary is dynamically linked only against each OS's LGPL-compatible base
system libraries (libc, CoreFoundation, …); no GPL code is linked.
`LICENSE` text of the LGPL-2.1 is distributed alongside the sidecar in the
bundle (`licenses/` directory of the packaged app).

## Verification

- `check.sh <ffmpeg>` runs the shipped binary itself: `-version`
  configuration flags and `-encoders` list must be free of GPL/nonfree
  entries, and the fallback encoders must be present.
- CI (`.github/workflows/ffmpeg-sidecar.yml`) builds and checks the sidecar
  on all three OSes for every PR touching this directory and for release
  tags.
- `fotobase-video::license_check` (crates/video) encodes the same rules in
  Rust; its unit tests pin the verdict rules.
- The version pin is checked: `check.sh` prints the version line and CI
  greps it against the pinned version.

## Bundling (Tauri sidecar mechanism)

The release workflow runs `tools/ffmpeg-sidecar/build.sh` into
`app/src-tauri/binaries/`, then `tauri.conf.json`'s `bundle.externalBin`
picks up `binaries/ffmpeg` with the per-target triple suffix Tauri requires
(`ffmpeg-x86_64-pc-windows-msvc.exe`, `ffmpeg-aarch64-apple-darwin`,
`ffmpeg-x86_64-unknown-linux-gnu`, …). The wire-up lands with the first
consumer task (02-004/02-006); this task pins the source, the build and the
license verification.
