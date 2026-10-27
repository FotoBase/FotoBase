//! `stream://` URI scheme — on-the-fly transcode fallback (TASK-02-006,
//! PLAN §2 Video strategy).
//!
//! When the codec probe (02-005) says the WebView cannot decode a clip, the
//! player points `<video>` at `stream://<file-id>`; this scheme transcodes
//! the clip with the ffmpeg sidecar into a supported intermediate
//! (H.264/mp4 preferred, VP9/webm fallback) and pipes it to the response.
//! Originals are only ever read; no media bytes cross IPC (PLAN §3 rule 3 —
//! the bytes travel through the scheme handler, same as `thumb://`).
//!
//! # Streaming model (and the platform constraint behind it)
//!
//! The functional spec asks for playback start within ~2 s, streamed output,
//! and no full-file temp copy. Tauri/wry deliver custom-scheme response
//! bodies as a single buffer on every platform (wry 0.57: `MemoryInputStream`
//! on WebKitGTK, one-shot `WKURLSchemeTask` on macOS, buffered
//! `WebResourceRequested` on Windows), so a byte-by-byte HTTP stream through
//! the scheme is not expressible. The scheme therefore serves *segments*:
//!
//! - Each `stream://<id>` (or `stream://<id>?t=<T>`) request runs one
//!   transcode from a keyframe at or before `T` and responds with its output
//!   up to the first of: EOF (short clips: the whole clip), the byte cap
//!   ([`SEGMENT_BYTE_CAP`]), or the first-bytes budget
//!   ([`FIRST_BYTES_BUDGET`]). Fragmented output (`-movflags
//!   frag_keyframe+empty_moov`) makes every response independently playable.
//! - The transcode process never outlives its response: it exits on EOF or
//!   is killed and reaped when the response is served. A player that needs
//!   more of a long clip re-requests at the current position (the seek
//!   mechanism — 02-007 consumes this).
//! - The [`StreamRegistry`] keyed by file-id kills in-flight transcodes when
//!   a request supersedes one, when the player stops (`stop_stream`
//!   command), and as the janitor's idle-timeout backstop. No orphan ffmpeg.
//!
//! Output is buffered in memory only, bounded by the cap — no temp file is
//! ever written to disk.

use std::collections::HashMap;
use std::io::{ErrorKind, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::codec::CodecSupport;
use tauri::http::Uri;

/// The custom URI scheme name registered in the Tauri builder.
pub const STREAM_PROTOCOL: &str = "stream";

/// Upper bound on the bytes buffered for one response (≈ 8 MiB). RAM guard:
/// the transcode is piped, never written to disk, so the cap is what keeps
/// the 150 MB idle-RAM budget safe during playback.
pub const SEGMENT_BYTE_CAP: usize = 8 * 1024 * 1024;

/// Wall-clock budget for producing the first bytes of a response. The
/// functional spec promises playback within ~2 s; serving happens at the
/// first of EOF, byte cap, or this budget.
pub const FIRST_BYTES_BUDGET: Duration = Duration::from_millis(1500);

/// Janitor backstop: a registered transcode older than this is killed and
/// reaped even if nobody stopped it (a hung child, an aborted request).
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// How often the janitor sweeps the registry.
pub const JANITOR_INTERVAL: Duration = Duration::from_secs(15);

/// A parsed `stream://<file-id>[?t=<seconds>]` request.
///
/// Both WebView spellings of a custom scheme are accepted, mirroring
/// `thumb://`: `stream://<id>` (macOS/Linux authority form),
/// `stream://localhost/<id>`, and `http://stream.localhost/<id>` (Windows
/// WebView2). `t` is the seek offset: the transcode restarts at a keyframe
/// at or before it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamRequest {
    pub file_id: i64,
    pub start_secs: Option<f64>,
}

/// Extract the request from a scheme request URI; `None` for anything that
/// is not a well-formed file id (the caller serves a 404 — never an error
/// dialog; ids are positive integers, so this doubles as the traversal
/// guard).
pub fn request_from_uri(uri: &Uri) -> Option<StreamRequest> {
    let host = uri.host().unwrap_or("");
    let path = uri.path().trim_start_matches('/');
    let id_str = if path.is_empty() {
        // Direct form `stream://<id>`: the id is the authority.
        host
    } else if host == "stream.localhost" || host == "localhost" {
        // Windows form; only the first path segment counts.
        path.split('/').next()?
    } else {
        return None;
    };
    let file_id: i64 = id_str.parse().ok()?;
    if file_id <= 0 {
        return None;
    }

    // Query: only `t=<finite non-negative seconds>` is meaningful; unknown
    // parameters are ignored.
    let start_secs = match uri.query() {
        None => None,
        Some(query) => {
            let mut start = None;
            for pair in query.split('&') {
                if let Some(value) = pair.strip_prefix("t=") {
                    let t: f64 = value.parse().ok()?;
                    if !t.is_finite() || t < 0.0 {
                        return None;
                    }
                    start = Some(t);
                }
            }
            start
        }
    };

    Some(StreamRequest { file_id, start_secs })
}

/// The transcode target: the intermediate format the WebView was probed to
/// support (task spec: "H.264/mp4 preferred, VP9/webm fallback").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// H.264 video + AAC audio in MP4.
    Mp4H264,
    /// VP9 video + Opus audio in WebM.
    WebmVp9,
}

impl Target {
    /// Content type of the served stream.
    pub fn content_type(&self) -> &'static str {
        match self {
            Target::Mp4H264 => "video/mp4",
            Target::WebmVp9 => "video/webm",
        }
    }
}

/// Pick the transcode target from the WebView's probe matrix. H.264/mp4 is
/// preferred (most widely decodable intermediate); VP9/webm is the fallback
/// for WebViews without H.264 (WebKitGTK without the restricted codecs). An
/// unprobed or fully-unsupported matrix still serves a transcode, defaulting
/// to the H.264/mp4 best effort.
pub fn choose_target(support: Option<&CodecSupport>) -> Target {
    match support {
        Some(s) if s.h264 => Target::Mp4H264,
        Some(s) if s.vp9 => Target::WebmVp9,
        _ => Target::Mp4H264,
    }
}

/// The ffmpeg argument vector for one transcode segment: decode the source,
/// re-encode into the target intermediate, and write fragmented output to
/// stdout (`-`), never to a file.
///
/// Input seek (`-ss` before `-i`) lands on the keyframe at or before the
/// requested time — the task's "keyframe near the seek point" — and ffmpeg's
/// default autorotation bakes the display matrix into the re-encode, so the
/// player need not honor rotation for streamed clips (02-007 honors it only
/// on the HTML5 path).
///
/// Encoders match the shipped LGPL sidecar (`tools/ffmpeg-sidecar/README.md`):
/// `libopenh264` (BSD) for H.264, `libvpx-vp9` for VP9 — checked in
/// `check.sh`; native `aac` for mp4 audio, `libopus` for webm audio.
pub fn transcode_args(src: &str, target: Target, start_secs: Option<f64>) -> Vec<String> {
    let mut args: Vec<String> =
        ["-hide_banner", "-loglevel", "error"].iter().map(|s| s.to_string()).collect();
    if let Some(t) = start_secs {
        args.push("-ss".into());
        args.push(format!("{t:.3}"));
    }
    args.push("-i".into());
    args.push(src.into());
    match target {
        Target::Mp4H264 => {
            args.extend([
                "-c:v".into(),
                "libopenh264".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-c:a".into(),
                "aac".into(),
                "-b:a".into(),
                "128k".into(),
                // Fragmented mp4: the moov is written up front and fragments
                // are cut at keyframes, so a response truncated at the cap or
                // the budget is still a playable stream.
                "-movflags".into(),
                "frag_keyframe+empty_moov".into(),
                "-f".into(),
                "mp4".into(),
            ]);
        }
        Target::WebmVp9 => {
            args.extend([
                "-c:v".into(),
                "libvpx-vp9".into(),
                // Constrained quality (constant quality at a sane rate).
                "-b:v".into(),
                "0".into(),
                "-crf".into(),
                "36".into(),
                // Speed-oriented settings: the budget is ~2 s to first bytes.
                "-deadline".into(),
                "realtime".into(),
                "-cpu-used".into(),
                "8".into(),
                "-c:a".into(),
                "libopus".into(),
                "-b:a".into(),
                "96k".into(),
                "-f".into(),
                "webm".into(),
            ]);
        }
    }
    args.push("-".into());
    args
}

/// One registered transcode: the child process and when it was started (the
/// janitor's idle clock). The child is owned here so [`StreamRegistry::kill`]
/// can kill *and reap* it — no zombie, no orphan.
struct Entry {
    child: Child,
    started: Instant,
}

/// Registry of in-flight transcode processes, keyed by file-id (task spec).
/// The Tauri shell holds one instance; `stop_stream` and the janitor go
/// through it.
#[derive(Default)]
pub struct StreamRegistry {
    entries: Mutex<HashMap<i64, Entry>>,
}

impl StreamRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a child for `file_id`, killing and reaping any child already
    /// registered for it (a new request supersedes an in-flight one).
    pub fn track(&self, file_id: i64, child: Child) {
        if let Some(mut previous) = self
            .entries
            .lock()
            .ok()
            .and_then(|mut g| g.insert(file_id, Entry { child, started: Instant::now() }))
        {
            let _ = previous.child.kill();
            let _ = previous.child.wait();
        }
    }

    /// Take the child out of the registry (ownership returns to the caller —
    /// used by [`serve`] after the response is built, to reap or kill it).
    pub fn take(&self, file_id: i64) -> Option<Child> {
        self.entries.lock().ok()?.remove(&file_id).map(|e| e.child)
    }

    /// Kill the transcode for `file_id` (player stop/destroy): kill + wait,
    /// then drop the entry. `true` when something was actually killed.
    pub fn kill(&self, file_id: i64) -> bool {
        let Some(mut entry) = self.entries.lock().ok().and_then(|mut g| g.remove(&file_id)) else {
            return false;
        };
        let _ = entry.child.kill();
        // Reap: the child must not linger as a zombie or orphan (functional
        // spec: "no orphan ffmpeg").
        let _ = entry.child.wait();
        true
    }

    /// Janitor backstop: kill and reap every entry that started at least
    /// `older_than` ago. Returns how many were killed.
    pub fn kill_idle(&self, older_than: Duration) -> usize {
        let stale: Vec<i64> = match self.entries.lock() {
            Ok(guard) => guard
                .iter()
                .filter(|(_, e)| e.started.elapsed() >= older_than)
                .map(|(id, _)| *id)
                .collect(),
            Err(_) => return 0,
        };
        let mut killed = 0;
        for id in stale {
            if self.kill(id) {
                killed += 1;
            }
        }
        killed
    }

    /// Number of in-flight transcodes (tests and diagnostics).
    pub fn len(&self) -> usize {
        self.entries.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Whether no transcode is in flight.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Everything the scheme handler needs to serve one request. The catalog
/// lookup (file-id → path) happens in the shell; this struct carries the
/// resolved values so [`serve`] stays catalog-free and testable.
#[derive(Debug, Clone)]
pub struct StreamJob {
    pub bin: String,
    pub src: PathBuf,
    pub target: Target,
    pub start_secs: Option<f64>,
    pub file_id: i64,
}

/// An HTTP-shaped reply the Tauri scheme handler turns into a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamReply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

/// Serve one `stream://` request: supersede any in-flight transcode for the
/// file, spawn the sidecar transcode, drain its stdout up to the byte cap
/// and the first-bytes budget, then make sure the process is reaped — the
/// response never outlives its child.
pub fn serve(job: &StreamJob, registry: &StreamRegistry) -> StreamReply {
    // A second request for the same file (seek, re-request) supersedes the
    // previous transcode: kill it before spawning a new one.
    registry.kill(job.file_id);

    let started = Instant::now();
    let mut child = match Command::new(&job.bin)
        .args(transcode_args(&job.src.to_string_lossy(), job.target, job.start_secs))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            tracing::warn!("stream transcode spawn failed for {}: {e}", job.src.display());
            return StreamReply { status: 500, content_type: "text/plain", body: Vec::new() };
        }
    };
    let out = child.stdout.take().expect("stdout is piped");
    registry.track(job.file_id, child);

    let body = drain(out, SEGMENT_BYTE_CAP, started + FIRST_BYTES_BUDGET);

    // The response is complete: nothing may keep running. EOF means the
    // child already exited (take + wait reaps it); a capped or budget-bound
    // drain means it is still producing — kill and reap it.
    if let Some(mut child) = registry.take(job.file_id) {
        let _ = child.kill();
        let _ = child.wait();
    }

    if body.is_empty() {
        // The transcode produced nothing (bad source, unsupported codec):
        // an error reply the player surfaces, never a silent empty stream.
        StreamReply { status: 500, content_type: "text/plain", body: Vec::new() }
    } else {
        StreamReply { status: 200, content_type: job.target.content_type(), body }
    }
}

/// Read the transcode output until EOF, the byte cap, or the deadline —
/// whichever comes first. This is what makes the scheme "stream-shaped"
/// under a one-shot response body: short clips arrive complete, long clips
/// arrive as a bounded prefix within the first-bytes budget.
///
/// The deadline must hold even while a read blocks (a hung child produces
/// nothing), so a reader thread does the blocking reads and the caller waits
/// on a channel with the remaining budget. After the deadline the reader may
/// still be parked in `read`; the caller kills the child (registry kill or
/// the post-response reap), the pipe closes and the thread exits — nothing
/// leaks. Cancellation is not a flag: killing the child closes the pipe and
/// an aborted request unblocks naturally.
pub fn drain(src: impl Read + Send + 'static, cap: usize, deadline: Instant) -> Vec<u8> {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut src = src;
        let mut buf = vec![0u8; 64 * 1024];
        let mut sent = 0usize;
        while sent < cap {
            let want = buf.len().min(cap - sent);
            match src.read(&mut buf[..want]) {
                Ok(0) => break, // EOF: the transcode finished.
                Ok(n) => {
                    sent += n;
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break; // Receiver gone: response already served.
                    }
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => break, // Pipe closed (child killed or died).
            }
        }
    });

    let mut body = Vec::new();
    while body.len() < cap {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok(chunk) => body.extend_from_slice(&chunk),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    body
}
