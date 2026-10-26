//! Live ffprobe integration (TASK-02-003): when real ffprobe/ffmpeg binaries
//! are available (CI runners have them), a generated tiny clip probes to the
//! expected values end-to-end through the scanner. Skipped when no binary —
//! the fixture-parsing and fake-prober tests cover all branches deterministically.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::Catalog;
use fotobase_scanner::{scan_folder_with_probes, ScanOptions};

fn bin_available(cmd: &str) -> bool {
    let check = if cfg!(windows) { "where" } else { "which" };
    Command::new(check).arg(cmd).output().map(|o| o.status.success()).unwrap_or(false)
}

fn ffmpeg_bin() -> Option<String> {
    if let Ok(v) = std::env::var("FB_FFMPEG") {
        if !v.is_empty() {
            return Some(v);
        }
    }
    bin_available("ffmpeg").then(|| "ffmpeg".to_owned())
}

/// Generate a tiny rotated h264 mp4 via ffmpeg (transpose filter so the
/// display-matrix rotation side data is written). Generated data is never
/// committed — created in a tempdir at test time.
fn generate_clip(dir: &Path, out: &str, rotate: bool) -> PathBuf {
    let path = dir.join(out);
    let mut cmd = Command::new(ffmpeg_bin().unwrap());
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc=duration=1:size=128x96:rate=10",
    ])
    .args(["-c:v", "libopenh264"])
    .args(["-y"]);
    if rotate {
        cmd.args(["-vf", "transpose=1"]);
    }
    let status = cmd.arg(&path).status().expect("run ffmpeg");
    assert!(status.success(), "ffmpeg clip generation failed");
    path
}

#[test]
fn live_probe_and_scan_roundtrip() {
    let Some(_ffmpeg) = ffmpeg_bin() else {
        eprintln!("skipping: no ffmpeg on PATH");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    generate_clip(tmp.path(), "plain.mp4", false);

    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
    let report = scan_folder_with_probes(
        &mut cat,
        tmp.path(),
        &AtomicBool::new(false),
        &ScanOptions::default(),
        &|_| {},
        &fotobase_scanner::DefaultVideoProbe,
    )
    .unwrap();
    assert_eq!(report.added, 1);

    let meta = cat
        .conn()
        .query_row(
            "SELECT m.video_duration > 0, m.video_codec, m.video_probe_ok FROM metadata m
             JOIN files f ON f.id = m.file_id WHERE f.filename = 'plain.mp4'",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)),
        )
        .unwrap();
    assert_eq!(meta.1, "h264", "ffmpeg's default encoder here is h264 (libopenh264 or native)");
    assert_eq!(meta.0, 1);
    assert_eq!(meta.2, 1);
}

#[test]
fn live_probe_marks_corrupt_file() {
    if !bin_available("ffprobe") {
        eprintln!("skipping: no ffprobe on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("corrupt.mp4"), vec![0u8; 64]).unwrap();
    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
    let report = scan_folder_with_probes(
        &mut cat,
        tmp.path(),
        &AtomicBool::new(false),
        &ScanOptions::default(),
        &|_| {},
        &fotobase_scanner::DefaultVideoProbe,
    )
    .unwrap();
    assert_eq!(report.added, 1, "scan succeeds despite the corrupt file");
    let marked: i64 = cat
        .conn()
        .query_row(
            "SELECT m.video_probe_ok FROM metadata m JOIN files f ON f.id = m.file_id
             WHERE f.filename = 'corrupt.mp4'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(marked, 0, "corrupt file is marked, not fatal");
}
