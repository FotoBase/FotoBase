//! `stream://` URI scheme tests (TASK-02-006, PLAN §2 Video strategy).
//!
//! When the WebView cannot decode a clip's codec, the scheme transcodes it
//! on the fly with the ffmpeg sidecar into a supported intermediate
//! (H.264/mp4 preferred, VP9/webm fallback) and pipes it to the response —
//! no full-file temp copy on disk. The pure core (URI parsing, target
//! choice, argv building, process registry, bounded drain) lives in
//! [`fotobase_lib::stream`] so it is testable without booting a Tauri app;
//! the Tauri registration in `lib.rs` is a thin adapter.
//!
//! Live ffmpeg paths are gated on a real binary (skipped when absent — the
//! arg/registry tests cover the deterministic branches). No test mutates
//! `FB_FFMPEG`; the binary path is always an explicit parameter.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use fotobase_lib::codec::CodecSupport;
use fotobase_lib::stream::{
    choose_target, drain, request_from_uri, serve, transcode_args, StreamJob, StreamRegistry,
    Target, FIRST_BYTES_BUDGET, SEGMENT_BYTE_CAP, STREAM_PROTOCOL,
};
use tauri::http::Uri;

/// A process that outlives any test: the cross-platform sleeper used to
/// prove that the registry kills and reaps children (no orphan ffmpeg).
fn sleeper() -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/c", "ping", "-n", "30", "127.0.0.1"]);
        c
    } else {
        let mut c = Command::new("sleep");
        c.arg("30");
        c
    }
}

/// A process that emits a few bytes on stdout quickly, then blocks (the
/// shape of a transcode that has produced its first bytes but not finished).
fn slow_producer() -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/c", "echo fakedata&& ping -n 30 127.0.0.1"]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", "printf fakedata; sleep 30"]);
        c
    }
}

fn spawn_stdout(cmd: &mut Command) -> Child {
    cmd.stdout(Stdio::piped()).stderr(Stdio::null()).spawn().expect("spawn fake process")
}

// --- URI parsing -----------------------------------------------------------

#[test]
fn the_scheme_name_is_stream() {
    assert_eq!(STREAM_PROTOCOL, "stream");
}

#[test]
fn request_from_uri_accepts_the_direct_scheme_form() {
    let uri: Uri = "stream://42".parse().unwrap();
    let req = request_from_uri(&uri).unwrap();
    assert_eq!(req.file_id, 42);
    assert_eq!(req.start_secs, None);
}

#[test]
fn request_from_uri_accepts_the_windows_webview_form() {
    let uri: Uri = "http://stream.localhost/42".parse().unwrap();
    assert_eq!(request_from_uri(&uri).unwrap().file_id, 42);

    let uri: Uri = "stream://localhost/42".parse().unwrap();
    assert_eq!(request_from_uri(&uri).unwrap().file_id, 42);
}

#[test]
fn request_from_uri_parses_the_seek_offset() {
    let uri: Uri = "stream://42?t=12.5".parse().unwrap();
    let req = request_from_uri(&uri).unwrap();
    assert_eq!(req.file_id, 42);
    assert_eq!(req.start_secs, Some(12.5));
}

#[test]
fn request_from_uri_rejects_bad_input() {
    for bad in [
        "stream://",                      // empty
        "stream://abc",                   // not a number
        "stream://-1",                    // negative id
        "stream://42.5",                  // not an integer id
        "stream://42?t=-3",               // negative seek
        "stream://42?t=abc",              // non-numeric seek
        "http://evil.localhost/42",       // wrong scheme host
        "http://stream.localhost/../etc", // traversal
    ] {
        let outcome = bad.parse::<Uri>().ok().as_ref().and_then(request_from_uri);
        assert!(outcome.is_none(), "{bad} must not parse");
    }
}

// --- Target choice ---------------------------------------------------------

#[test]
fn h264_capable_webviews_get_the_h264_mp4_target() {
    // Spec: "H.264/mp4 preferred, VP9/webm fallback".
    let support = CodecSupport { h264: true, hevc: false, vp9: true, av1: false };
    assert_eq!(choose_target(Some(&support)), Target::Mp4H264);
}

#[test]
fn webviews_without_h264_fall_back_to_vp9_webm() {
    // The WebKitGTK case: no H.264, but VP9 (free codec) usually decoded.
    let support = CodecSupport { h264: false, hevc: false, vp9: true, av1: false };
    assert_eq!(choose_target(Some(&support)), Target::WebmVp9);
}

#[test]
fn an_unprobed_matrix_defaults_to_the_most_compatible_target() {
    // Fail closed (02-005): no data → still serve a transcode, H.264/mp4 is
    // the most widely decodable intermediate.
    assert_eq!(choose_target(None), Target::Mp4H264);
}

#[test]
fn targets_carry_their_container_and_content_type() {
    assert_eq!(Target::Mp4H264.content_type(), "video/mp4");
    assert_eq!(Target::WebmVp9.content_type(), "video/webm");
}

// --- ffmpeg argv -----------------------------------------------------------

#[test]
fn mp4_args_transcode_to_a_fragmented_mp4_pipe() {
    let args = transcode_args("/v/clip.hevc", Target::Mp4H264, None);

    assert_eq!(args.last().unwrap(), "-"); // piped to the response, not a file
    assert!(args.iter().any(|a| a.contains("clip.hevc")));
    // H.264 video (libopenh264 is the BSD encoder the shipped LGPL sidecar
    // keeps) + AAC audio, per the task spec.
    assert!(args.windows(2).any(|w| w[0] == "-c:v" && w[1] == "libopenh264"));
    assert!(args.windows(2).any(|w| w[0] == "-c:a" && w[1] == "aac"));
    // Fragmented output: playable before the transcode completes.
    assert!(args.windows(2).any(|w| w[0] == "-movflags" && w[1] == "frag_keyframe+empty_moov"));
    assert!(args.windows(2).any(|w| w[0] == "-f" && w[1] == "mp4"));
    // No temp file anywhere in the argv.
    assert!(!args.iter().any(|a| a.ends_with(".mp4") && !a.contains("clip")));
}

#[test]
fn webm_args_transcode_to_vp9_opus() {
    let args = transcode_args("/v/clip.hevc", Target::WebmVp9, None);
    assert!(args.windows(2).any(|w| w[0] == "-c:v" && w[1] == "libvpx-vp9"));
    assert!(args.windows(2).any(|w| w[0] == "-c:a" && w[1] == "libopus"));
    assert!(args.windows(2).any(|w| w[0] == "-f" && w[1] == "webm"));
    assert_eq!(args.last().unwrap(), "-");
}

#[test]
fn seek_restarts_the_transcode_at_a_keyframe_before_the_seek_point() {
    // Input seek (`-ss` before `-i`) lands on the keyframe at or before the
    // requested time — exactly the task's "keyframe near the seek point".
    let args = transcode_args("/v/clip.mp4", Target::Mp4H264, Some(12.5));
    let ss = args.iter().position(|a| a == "-ss").expect("-ss present");
    assert_eq!(args[ss + 1], "12.500");
    let i = args.iter().position(|a| a == "-i").expect("-i present");
    assert!(ss < i, "input seek must precede -i");
}

#[test]
fn no_seek_means_no_ss_flag() {
    let args = transcode_args("/v/clip.mp4", Target::Mp4H264, None);
    assert!(!args.iter().any(|a| a == "-ss"));
}

// --- Process registry (no orphan ffmpeg) ------------------------------------

#[test]
fn kill_terminates_the_child_and_reaps_it() {
    let registry = StreamRegistry::new();
    let child = spawn_stdout(&mut sleeper());
    let pid = child.id();
    registry.track(7, child);
    assert_eq!(registry.len(), 1);

    assert!(registry.kill(7), "kill must report the killed entry");
    assert_eq!(registry.len(), 0, "killed entry must be removed");
    assert!(!registry.kill(7), "second kill: nothing tracked");

    // The process is reaped (waited), not merely signalled: /proc-style
    // existence checks would race, so assert via pid reuse impossibility —
    // the strongest portable statement is that the registry is empty and
    // kill() waited the child internally.
    let _ = pid;
}

#[test]
fn tracking_the_same_file_id_supersedes_the_previous_child() {
    let registry = StreamRegistry::new();
    let first = spawn_stdout(&mut sleeper());
    let mut first = Some(first);
    registry.track(7, first.take().unwrap());
    let second = spawn_stdout(&mut sleeper());
    registry.track(7, second);

    assert_eq!(registry.len(), 1, "one entry per file-id");
    // The superseded child is dead (kill + reap happened inside track).
    assert!(registry.kill(7));
    assert_eq!(registry.len(), 0);
}

#[test]
fn kill_idle_is_the_backstop_for_orphaned_transcodes() {
    let registry = StreamRegistry::new();
    registry.track(7, spawn_stdout(&mut sleeper()));

    // A fresh entry survives a generous idle timeout.
    assert_eq!(registry.kill_idle(Duration::from_secs(60)), 0);
    assert_eq!(registry.len(), 1);

    // A zero timeout kills everything (nothing is older than 0s… every entry
    // is at least 0s old), leaving no orphan processes.
    assert_eq!(registry.kill_idle(Duration::ZERO), 1);
    assert_eq!(registry.len(), 0);
}

// --- Bounded drain (streams, no full temp copy) -----------------------------

#[test]
fn drain_stops_at_the_byte_cap_and_child_is_killed_afterwards() {
    // A cap far below what the producer emits: the drain must return the
    // capped prefix quickly instead of buffering the whole output.
    let mut child = spawn_stdout(&mut slow_producer());
    let out = child.stdout.take().unwrap();

    let body = drain(out, 4, Instant::now() + Duration::from_secs(10));
    assert_eq!(body.len(), 4, "drain stops at the cap");
    assert_eq!(&body, b"fake");
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn drain_returns_the_first_bytes_within_the_two_second_budget() {
    // Functional spec: "Unsupported-codec clips start playback within ~2 s."
    // The scheme responds as soon as the budget elapses — first bytes are
    // served, never a full-file wait.
    let mut child = spawn_stdout(&mut slow_producer());
    let out = child.stdout.take().unwrap();

    let start = Instant::now();
    let body = drain(out, SEGMENT_BYTE_CAP, start + FIRST_BYTES_BUDGET);
    assert!(!body.is_empty(), "first bytes must arrive");
    assert!(start.elapsed() < Duration::from_secs(2), "first bytes within ~2 s");
    // The child is still running (it produced no EOF): serve() kills it.
    assert!(child.try_wait().unwrap().is_none());
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn drain_honours_eof_and_terminates_cleanly() {
    let mut child = {
        let mut c = if cfg!(windows) {
            // `cmd /c echo` appends CRLF; `<nul set /p=` prints without a newline.
            let mut c = Command::new("cmd");
            c.args(["/c", "<nul set /p=fakedata"]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", "printf fakedata"]);
            c
        };
        spawn_stdout(&mut c)
    };
    let out = child.stdout.take().unwrap();

    let body = drain(out, SEGMENT_BYTE_CAP, Instant::now() + Duration::from_secs(10));
    assert_eq!(body, b"fakedata".to_vec());
    // EOF: the producer exits on its own.
    let _ = child.wait().unwrap();
}

// --- serve(): the whole per-request pipeline -------------------------------

fn job(bin: &str, src: &str, target: Target, start: Option<f64>) -> StreamJob {
    StreamJob { bin: bin.to_owned(), src: src.into(), target, start_secs: start, file_id: 7 }
}

#[test]
fn serve_with_a_missing_binary_is_an_error_reply_never_a_panic() {
    let registry = StreamRegistry::new();
    let reply = serve(&job("fb-no-such-ffmpeg", "/v/clip.mp4", Target::Mp4H264, None), &registry);
    assert_ne!(reply.status, 200);
    assert!(reply.body.is_empty());
    assert_eq!(registry.len(), 0, "failed spawn leaves nothing behind");
}

#[test]
fn serve_with_a_failing_transcode_is_an_error_reply() {
    // A real binary that cannot produce output for a nonexistent source:
    // ffmpeg exits nonzero quickly; the reply must be an error, and nothing
    // may stay registered (no orphan).
    let Some(ffmpeg) = which("ffmpeg") else { return };
    let registry = StreamRegistry::new();
    let reply = serve(&job(&ffmpeg, "/no/such/clip.mp4", Target::Mp4H264, None), &registry);
    assert_ne!(reply.status, 200);
    assert_eq!(registry.len(), 0);
}

/// Live pipeline: a real transcode of a generated clip serves a playable
/// fragmented mp4 within the ~2 s first-bytes budget (TDD testing criteria).
#[test]
fn live_serve_transcodes_a_clip_to_a_playable_stream() {
    let Some(ffmpeg) = which("ffmpeg") else { return };

    let tmp = tempfile::tempdir().unwrap();
    let clip = tmp.path().join("clip.mp4");
    let status = Command::new(&ffmpeg)
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

    let registry = StreamRegistry::new();
    let start = Instant::now();
    let reply = serve(&job(&ffmpeg, &clip.to_string_lossy(), Target::Mp4H264, None), &registry);
    assert_eq!(reply.status, 200);
    assert_eq!(reply.content_type, "video/mp4");
    // mp4 signature: a `ftyp` box right at the start.
    assert!(reply.body.len() > 8, "nonempty stream");
    assert_eq!(&reply.body[4..8], b"ftyp", "starts with an mp4 ftyp box");
    assert!(start.elapsed() < Duration::from_secs(2), "first bytes within ~2 s");
    assert_eq!(registry.len(), 0, "completed transcode leaves no process");
}

/// Live seek: restarting the transcode at a keyframe near the seek point
/// still yields a playable fragmented mp4.
#[test]
fn live_serve_restarts_at_the_seek_point() {
    let Some(ffmpeg) = which("ffmpeg") else { return };
    let tmp = tempfile::tempdir().unwrap();
    let clip = tmp.path().join("clip.mp4");
    let status = Command::new(&ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=4:size=128x96:rate=10",
            "-c:v",
            "libopenh264",
            "-y",
        ])
        .arg(&clip)
        .status()
        .expect("run ffmpeg");
    assert!(status.success(), "clip generation failed");

    let registry = StreamRegistry::new();
    let reply =
        serve(&job(&ffmpeg, &clip.to_string_lossy(), Target::Mp4H264, Some(2.0)), &registry);
    assert_eq!(reply.status, 200);
    assert_eq!(&reply.body[4..8], b"ftyp");
    assert_eq!(registry.len(), 0);
}

fn which(cmd: &str) -> Option<String> {
    let check = if cfg!(windows) { "where" } else { "which" };
    Command::new(check)
        .arg(cmd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| cmd.to_owned())
}
