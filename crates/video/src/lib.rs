//! ffprobe/ffmpeg integration (PLAN §3 `video/`, §2 Video strategy).
//!
//! Scan time: [`Ffprobe::probe`] extracts duration, codec (name + profile)
//! and display rotation from a video file; the scanner stores the result in
//! the catalog `metadata` table. Playback honors rotation later (TASK-02-007).
//!
//! Robustness (PLAN §9): a missing binary, a nonzero exit, or unparseable
//! output is an [`Err`] — never a panic — so the scanner can log it and mark
//! the file instead of dying. Malformed media must never crash the scan.
//!
//! Licensing (§10 risk row): the shipped ffmpeg sidecar must be an LGPL build
//! with no non-free codecs; [`license_check`] asserts that from the binary
//! itself (`-version` config flags + `-encoders` list) and CI runs it against
//! the pinned downloads from `tools/ffmpeg-sidecar/`.

use std::path::Path;
use std::process::Command;

use serde::Deserialize;

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-video";

/// Scan-time video metadata extracted by ffprobe (PLAN §2 Video strategy:
/// "extracted at scan time via ffprobe, stored in the catalog `metadata`
/// table").
#[derive(Debug, Clone, PartialEq)]
pub struct VideoMeta {
    /// Duration in seconds (from the video stream, else the container).
    pub duration_secs: f64,
    /// Codec name, e.g. `h264`, `hevc`, `vp9`.
    pub codec: String,
    /// Codec profile, e.g. `High`, `Main 10` — `None` when ffprobe reports none.
    pub profile: Option<String>,
    /// Display rotation in degrees, normalized to 0/90/180/270.
    pub rotation_deg: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    /// ffprobe/ffmpeg could not be spawned (missing binary, no PATH).
    #[error("ffprobe/ffmpeg not runnable: {0}")]
    Spawn(String),
    /// The process ran but exited nonzero (corrupt/unsupported file).
    #[error("{tool} exited {:?}: {stderr}", code.as_ref())]
    #[allow(unused_qualifications)]
    Exit { tool: &'static str, code: Option<i32>, stderr: String },
    /// Output could not be parsed as expected (JSON, stream fields…).
    #[error("unparseable {0} output: {1}")]
    Parse(String, #[source] serde_json::Error),
    /// Parsed, but no usable video stream (audio-only or empty container).
    #[error("no video stream found")]
    NoVideoStream,
}

/// Anything that can answer "what is in this video file". Implemented by
/// [`Ffprobe`]; the scanner takes `&dyn VideoProbe` so tests can inject
/// fixtures and failure modes without a real binary.
pub trait VideoProbe {
    fn probe(&self, path: &Path) -> Result<VideoMeta, VideoError>;
}

/// ffprobe JSON output, only the fields we read. Unknown fields are ignored
/// (`serde` default) — ffprobe adds keys between versions and parsing must
/// not break when it does.
#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    profile: Option<String>,
    duration: Option<String>,
    /// Newer ffprobe: display-matrix rotation side data.
    #[serde(default)]
    side_data_list: Vec<SideData>,
    /// Older ffprobe / some containers: `tags.rotate`.
    tags: Option<ProbeTags>,
}

#[derive(Debug, Deserialize)]
struct SideData {
    rotation: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ProbeTags {
    rotate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

/// Locate a tool binary. `FB_FFPROBE` / `FB_FFMPEG` override the PATH lookup
/// (tests and CI pin the exact build; users with several installs can too).
fn tool_bin(env_key: &str, default: &str) -> (String, String) {
    match std::env::var(env_key) {
        Ok(v) if !v.is_empty() => (env_key.to_string(), v),
        _ => (env_key.to_string(), default.to_string()),
    }
}

/// ffprobe wrapper (JSON output mode, per the task's technical spec).
#[derive(Debug, Clone, Default)]
pub struct Ffprobe;

/// Frame extraction (TASK-02-004, PLAN §2 Video strategy thumbnails bullet):
/// pull one frame at ~10% of the clip's duration out of a video file as a
/// JPEG, to be resized into the shared thumbnail cache by the render crate.
///
/// The prober and the binary are injectable so tests can pin builds or fake
/// failure modes without touching the process environment. A missing binary,
/// a failed probe, or a nonzero ffmpeg exit is an [`Err`] — never a panic.
#[derive(Default)]
pub struct Ffmpeg {
    bin: Option<String>,
    prober: Option<Box<dyn VideoProbe + Send + Sync>>,
}

impl std::fmt::Debug for Ffmpeg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ffmpeg").field("bin", &self.bin).finish_non_exhaustive()
    }
}

/// The ffmpeg argument vector for one frame grab: seek to `duration * 0.1`
/// seconds (clamped to a finite, non-negative offset), decode a single video
/// frame, write it as a JPEG (`-q:v 2` ≈ high quality still).
pub fn frame_seek_args(duration_secs: f64, src: &Path, out: &Path) -> Vec<String> {
    let seek = if duration_secs.is_finite() { (duration_secs * 0.1).max(0.0) } else { 0.0 };
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-ss".into(),
        format!("{seek:.3}"),
        "-i".into(),
        src.to_string_lossy().into_owned(),
        "-frames:v".into(),
        "1".into(),
        "-q:v".into(),
        "2".into(),
        "-y".into(),
        out.to_string_lossy().into_owned(),
    ]
}

impl Ffmpeg {
    /// An extractor with an explicit binary path and prober (tests pin builds
    /// or fake failure modes without touching the shared environment).
    pub fn with_parts(bin: &str, prober: Box<dyn VideoProbe + Send + Sync>) -> Self {
        Ffmpeg { bin: Some(bin.to_owned()), prober: Some(prober) }
    }
}

/// Anything that can pull one preview frame out of a video file. Implemented
/// by [`Ffmpeg`]; the render crate takes `&dyn FrameExtractor` so tests can
/// fake extraction without a real binary.
pub trait FrameExtractor {
    fn extract_frame(&self, video: &Path, out: &Path) -> Result<(), VideoError>;
}

impl FrameExtractor for Ffmpeg {
    fn extract_frame(&self, video: &Path, out: &Path) -> Result<(), VideoError> {
        let prober: &dyn VideoProbe = self.prober.as_deref().unwrap_or(&Ffmpeg::DEFAULT_PROBER);
        let meta = prober.probe(video)?;
        let bin: String = match &self.bin {
            Some(b) => b.clone(),
            None => tool_bin("FB_FFMPEG", "ffmpeg").1,
        };
        let args = frame_seek_args(meta.duration_secs, video, out);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        run(&bin, "ffmpeg", &refs, None)?;
        Ok(())
    }
}

impl Ffmpeg {
    const DEFAULT_PROBER: Ffprobe = Ffprobe;
}

impl VideoProbe for Ffprobe {
    fn probe(&self, path: &Path) -> Result<VideoMeta, VideoError> {
        let (_, bin) = tool_bin("FB_FFPROBE", "ffprobe");
        let out = run(
            &bin,
            "ffprobe",
            &["-v", "error", "-print_format", "json", "-show_format", "-show_streams"],
            Some(path.as_os_str()),
        )?;
        parse_probe_json(&out)
    }
}

/// Parse one ffprobe JSON document into [`VideoMeta`]. Pure — unit tests feed
/// it fixtures recorded from real ffprobe runs.
pub fn parse_probe_json(json: &str) -> Result<VideoMeta, VideoError> {
    let doc: ProbeOutput =
        serde_json::from_str(json).map_err(|e| VideoError::Parse("ffprobe".into(), e))?;

    let stream = doc
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"))
        .ok_or(VideoError::NoVideoStream)?;

    let codec = stream
        .codec_name
        .clone()
        .or_else(|| stream.codec_type.clone())
        .ok_or(VideoError::NoVideoStream)?;

    let duration = stream
        .duration
        .as_deref()
        .or(doc.format.as_ref().and_then(|f| f.duration.as_deref()))
        .and_then(|d| d.parse::<f64>().ok())
        .ok_or(VideoError::NoVideoStream)?;

    let rotation_deg = stream
        .side_data_list
        .iter()
        .find_map(|s| s.rotation)
        .or_else(|| stream.tags.as_ref().and_then(|t| t.rotate.as_deref()).and_then(parse_deg))
        .map(normalize_deg)
        .unwrap_or(0);

    Ok(VideoMeta { duration_secs: duration, codec, profile: stream.profile.clone(), rotation_deg })
}

/// Legacy `tags.rotate` is a string ("90", "-90", "0"); parse to degrees.
fn parse_deg(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok()
}

/// Normalize any rotation to 0/90/180/270 (negative values wrap; -90 is a
/// common iOS display-matrix value and displays as 270 counter-clockwise —
/// playback only needs a consistent convention, which is documented here).
fn normalize_deg(deg: f64) -> u32 {
    let n = ((deg.round() as i64).rem_euclid(360)) as u32;
    match n {
        46..=134 => 90,
        136..=224 => 180,
        226..=314 => 270,
        _ => n,
    }
}

fn run(
    bin: &str,
    tool: &'static str,
    args: &[&str],
    last: Option<&std::ffi::OsStr>,
) -> Result<String, VideoError> {
    let mut cmd = Command::new(bin);
    cmd.args(args);
    if let Some(last) = last {
        cmd.arg(last);
    }
    let out = cmd.output().map_err(|e| VideoError::Spawn(format!("{bin}: {e}")))?;
    if !out.status.success() {
        return Err(VideoError::Exit {
            tool,
            code: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).trim_end().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Which encoders must NEVER appear in a shipped ffmpeg build. libx264 /
/// libx265 are GPL (§10: "ship LGPL build without non-free codecs"); the
/// others are the classic nonfree set.
const FORBIDDEN_ENCODERS: &[&str] = &[
    "libx264",
    "libx265",
    "libxvid",
    "libaom-av1",
    "libsvtav1",
    "nvenc",
    "h264_nvenc",
    "hevc_nvenc",
    "h264_amf",
    "hevc_amf",
    "h264_qsv",
    "hevc_qsv",
    "h264_videotoolbox",
    "hevc_videotoolbox",
];

const FORBIDDEN_CONFIG_FLAGS: &[&str] =
    &["--enable-gpl", "--enable-nonfree", "--enable-libx264", "--enable-libx265"];

/// A shipped ffmpeg's license posture, derived from the binary itself.
#[derive(Debug, PartialEq, Eq)]
pub struct LicenseReport {
    pub version: String,
    pub configuration: String,
    pub encoders: Vec<String>,
    pub forbidden_encoders: Vec<String>,
    pub forbidden_flags: Vec<String>,
}

impl LicenseReport {
    pub fn is_lgpl_clean(&self) -> bool {
        self.forbidden_encoders.is_empty() && self.forbidden_flags.is_empty()
    }
}

/// Run the sidecar ffmpeg's `-version` and `-encoders` and check it is an
/// LGPL build without non-free/GPL codecs. CI runs this against every pinned
/// download (`tools/ffmpeg-sidecar/`) — a build that quietly gained
/// `--enable-gpl` fails the check instead of shipping.
pub fn license_check(bin: &str) -> Result<LicenseReport, VideoError> {
    let version_out = run_raw(bin, "ffmpeg", &["-version"])?;
    let encoders_out = run_raw(bin, "ffmpeg", &["-hide_banner", "-encoders"])?;
    check_license_text(&version_out, &encoders_out, bin)
}

/// Pure form of [`license_check`]'s verdict, so the rules are unit-testable
/// against recorded `-version` / `-encoders` output.
pub fn check_license_text(
    version_out: &str,
    encoders_out: &str,
    bin: &str,
) -> Result<LicenseReport, VideoError> {
    let version = version_out.lines().next().unwrap_or_default().trim().to_owned();
    let configuration = version_out
        .lines()
        .find(|l| l.trim_start().starts_with("configuration:"))
        .map(|l| l.trim_start()[("configuration:".len())..].trim().to_owned())
        .unwrap_or_default();

    let encoders: Vec<String> = encoders_out
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_owned))
        .collect();

    let forbidden_encoders =
        encoders.iter().filter(|e| FORBIDDEN_ENCODERS.contains(&e.as_str())).cloned().collect();
    let forbidden_flags = FORBIDDEN_CONFIG_FLAGS
        .iter()
        .filter(|f| configuration.split_whitespace().any(|w| w == **f))
        .map(|f| f.to_string())
        .collect();

    if version.is_empty() {
        return Err(VideoError::Parse(
            bin.to_owned(),
            serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "empty ffmpeg -version output",
            )),
        ));
    }

    Ok(LicenseReport { version, configuration, encoders, forbidden_encoders, forbidden_flags })
}

fn run_raw(bin: &str, tool: &'static str, args: &[&str]) -> Result<String, VideoError> {
    let out = Command::new(bin)
        .args(args)
        .output()
        .map_err(|e| VideoError::Spawn(format!("{bin}: {e}")))?;
    if !out.status.success() {
        return Err(VideoError::Exit {
            tool,
            code: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).trim_end().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_identity() {
        assert_eq!(super::CRATE_NAME, "fotobase-video");
    }
}
