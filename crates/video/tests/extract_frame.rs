//! Frame extraction (TDD, task 02-004 testing criteria).
//!
//! The seek-point math is pure and unit-tested here; the ffmpeg invocation
//! is tested live (gated on a real binary — CI runners have one) by
//! generating a tiny clip and extracting a frame from it. Failure modes are
//! covered through injectable prober/binary: a prober that errors must yield
//! an `Err`, never a panic (PLAN §9). No test mutates `FB_FF*` env vars —
//! parallel tests share the process environment.

use std::path::Path;

use fotobase_video::{Ffmpeg, FrameExtractor, VideoError, VideoMeta, VideoProbe};

/// A prober stub returning a canned result (fresh error values per call —
/// `VideoError` is not `Clone`).
enum Stub {
    Ok(f64),
    Fail,
}

impl VideoProbe for Stub {
    fn probe(&self, _path: &Path) -> Result<VideoMeta, VideoError> {
        match self {
            Stub::Ok(duration) => Ok(meta(*duration)),
            Stub::Fail => Err(VideoError::NoVideoStream),
        }
    }
}

fn meta(duration: f64) -> VideoMeta {
    VideoMeta { duration_secs: duration, codec: "h264".into(), profile: None, rotation_deg: 0 }
}

#[test]
fn seek_point_is_ten_percent_of_duration() {
    // 10 s clip → frame at 1.0 s. The spec says "~10% of duration".
    let args =
        fotobase_video::frame_seek_args(10.0, Path::new("/v/clip.mp4"), Path::new("/o/f.jpg"));
    let ss = args.iter().position(|a| a == "-ss").expect("-ss present");
    let seek: f64 = args[ss + 1].parse().unwrap();
    assert!((seek - 1.0).abs() < 1e-9, "seek = {seek}");

    // The source and output paths appear, and a single video frame is
    // requested.
    assert!(args.iter().any(|a| a.contains("clip.mp4")));
    assert!(args.iter().any(|a| a.contains("f.jpg")));
    assert!(args.iter().any(|a| a == "-frames:v"));
    assert!(args.iter().any(|a| a == "1"));
}

#[test]
fn seek_point_clamps_to_a_nonnegative_offset() {
    // Degenerate durations must not produce a negative or NaN seek.
    for dur in [0.0, -5.0, f64::NAN] {
        let args = fotobase_video::frame_seek_args(dur, Path::new("a"), Path::new("b"));
        let ss = args.iter().position(|a| a == "-ss").unwrap();
        let seek: f64 = args[ss + 1].parse().unwrap();
        assert!(seek.is_finite() && seek >= 0.0, "dur {dur} → seek {seek}");
    }
}

#[test]
fn failed_probe_yields_err_not_panic() {
    let ffmpeg = Ffmpeg::with_parts("ffmpeg", Box::new(Stub::Fail));
    let out = std::env::temp_dir().join("fb-02-004-should-not-exist.jpg");
    let _ = std::fs::remove_file(&out);
    assert!(ffmpeg.extract_frame(Path::new("whatever.mp4"), &out).is_err());
}

#[test]
fn extractor_error_is_an_error_not_a_panic() {
    // A probe that succeeds but an ffmpeg binary that cannot run (a
    // nonexistent path is injected) must yield `Err`.
    let ffmpeg = Ffmpeg::with_parts("/nonexistent/fb-02-004-ffmpeg", Box::new(Stub::Ok(10.0)));
    let result = ffmpeg.extract_frame(Path::new("whatever.mp4"), &Path::new("/tmp").join("x.jpg"));
    assert!(matches!(result, Err(VideoError::Spawn(_))));
}

/// Live extraction against real ffmpeg (skipped when no binary — the
/// fixture/arg tests above cover the deterministic branches).
#[test]
fn live_extraction_produces_a_jpeg_frame() {
    let (ffmpeg_bin, ffprobe_bin) = match (which("ffmpeg"), which("ffprobe")) {
        (Some(f), Some(p)) => (f, p),
        _ => return, // no binaries: skip
    };

    let tmp = tempfile::tempdir().unwrap();
    let clip = tmp.path().join("clip.mp4");
    let status = std::process::Command::new(&ffmpeg_bin)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=128x96:rate=10",
            "-c:v",
            "libopenh264",
            "-y",
        ])
        .arg(&clip)
        .status()
        .expect("run ffmpeg");
    assert!(status.success(), "clip generation failed");

    let frame = tmp.path().join("frame.jpg");
    Ffmpeg::with_parts(&ffmpeg_bin, Box::new(PathProbe(ffprobe_bin)))
        .extract_frame(&clip, &frame)
        .expect("extract frame");

    let bytes = std::fs::read(&frame).expect("frame file written");
    assert!(bytes.starts_with(&[0xFF, 0xD8, 0xFF]), "frame is a JPEG");
    // Decodable and sized like the source.
    let img = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .unwrap()
        .decode()
        .expect("frame decodes");
    assert_eq!((img.width(), img.height()), (128, 96));
}

fn which(cmd: &str) -> Option<String> {
    let check = if cfg!(windows) { "where" } else { "which" };
    std::process::Command::new(check)
        .arg(cmd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| cmd.to_owned())
}

/// A prober that shells out to an ffprobe at an explicit path (the live test
/// pins binaries without touching the shared process environment).
struct PathProbe(String);

impl VideoProbe for PathProbe {
    fn probe(&self, path: &Path) -> Result<VideoMeta, VideoError> {
        use std::process::Command;
        let out = Command::new(&self.0)
            .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
            .arg(path)
            .output()
            .map_err(|e| VideoError::Spawn(format!("{}: {e}", self.0)))?;
        if !out.status.success() {
            return Err(VideoError::Exit {
                tool: "ffprobe",
                code: out.status.code(),
                stderr: String::from_utf8_lossy(&out.stderr).trim_end().to_owned(),
            });
        }
        fotobase_video::parse_probe_json(&String::from_utf8_lossy(&out.stdout))
    }
}
