//! LGPL / no-non-free-codec checks (TDD, task 02-003 testing criteria).
//!
//! The shipped ffmpeg sidecar must be an LGPL build with no non-free codecs;
//! CI runs [`fotobase_video::license_check`] against every pinned download.
//! These tests pin the verdict rules against recorded `-version` and
//! `-encoders` output, plus a live check when a real ffmpeg is on PATH.

use fotobase_video::{check_license_text, license_check};

const LGPL_VERSION: &str = "\
ffmpeg version 7.1-fotobase-1 Copyright (c) 2000-2024 the FFmpeg developers
configuration: --disable-gpl --disable-nonfree --enable-libvpx --enable-libopus";

const GPL_VERSION: &str = "\
ffmpeg version 7.1 Copyright (c) 2000-2024 the FFmpeg developers
configuration: --enable-gpl --enable-libx264 --enable-libx265";

const LGPL_ENCODERS: &str = "\
Encoders:
 V..... = Video
 A..... = Audio
 V....D libvpx-vp9         libvpx VP9 (codec vp9)
 V....D libvpx             libvpx VP8 (codec vp8)
 V....D libopenh264        OpenH264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10
 A....D libopus            libopus Opus
 A....D aac                AAC (Advanced Audio Coding)
";

const GPL_ENCODERS: &str = "\
Encoders:
 V..... = Video
 V....D libx264            libx264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10
 V....D libx265            libx265 H.265 / HEVC
 A....D aac                AAC (Advanced Audio Coding)
";

#[test]
fn lgpl_build_passes() {
    let report = check_license_text(LGPL_VERSION, LGPL_ENCODERS, "ffmpeg").unwrap();
    assert!(report.is_lgpl_clean(), "forbidden: {report:?}");
    assert_eq!(
        report.version,
        "ffmpeg version 7.1-fotobase-1 Copyright (c) 2000-2024 the FFmpeg developers"
    );
    assert!(report.encoders.contains(&"libvpx-vp9".to_owned()));
}

#[test]
fn gpl_build_fails_on_flag() {
    let report = check_license_text(GPL_VERSION, GPL_ENCODERS, "ffmpeg").unwrap();
    assert!(!report.is_lgpl_clean());
    assert!(report.forbidden_flags.contains(&"--enable-gpl".to_owned()));
    assert!(report.forbidden_encoders.contains(&"libx264".to_owned()));
    assert!(report.forbidden_encoders.contains(&"libx265".to_owned()));
}

#[test]
fn nonfree_flag_fails_even_without_gpl_encoders() {
    let out = "ffmpeg version 7.1\nconfiguration: --enable-nonfree";
    let report = check_license_text(out, LGPL_ENCODERS, "ffmpeg").unwrap();
    assert!(!report.is_lgpl_clean());
    assert_eq!(report.forbidden_flags, vec!["--enable-nonfree".to_owned()]);
}

#[test]
fn hardware_encoders_fail() {
    // Vendor SDK encoders (nvenc/qsv/amf/videotoolbox) are nonfree builds.
    let encoders = "Encoders:\n V....D h264_nvenc         NVIDIA NVENC H.264 encoder\n V....D hevc_videotoolbox  VideoToolbox H.265 encoder\n";
    let report = check_license_text(LGPL_VERSION, encoders, "ffmpeg").unwrap();
    assert!(!report.is_lgpl_clean());
    assert_eq!(report.forbidden_encoders.len(), 2);
}

/// Live check: when a real ffmpeg is on PATH (CI runners have one; the CI
/// sidecar job runs this against the pinned downloads), the *system* ffmpeg
/// would typically still pass or fail loudly — what must never happen is a
/// spawn error being swallowed.
#[test]
fn live_ffmpeg_reports_something() {
    let Ok(path) = which_ffmpeg() else {
        eprintln!("skipping: no ffmpeg on PATH");
        return;
    };
    let report = license_check(&path).unwrap();
    assert!(report.version.starts_with("ffmpeg version"));
}

fn which_ffmpeg() -> Result<String, ()> {
    if let Ok(v) = std::env::var("FB_FFMPEG") {
        if !v.is_empty() {
            return Ok(v);
        }
    }
    let out = if cfg!(windows) {
        std::process::Command::new("where").arg("ffmpeg").output()
    } else {
        std::process::Command::new("which").arg("ffmpeg").output()
    }
    .map_err(|_| ())?;
    if !out.status.success() {
        return Err(());
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.lines().next().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).ok_or(())
}
