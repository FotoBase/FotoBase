//! Startup codec-capability probe storage (TASK-02-005, PLAN §10: "WebKitGTK
//! ships without H.264/HEVC on several distros"). The WebView probes its own
//! decoders once at startup and reports the per-codec matrix through the
//! [`report_codec_support`] command; the player consults it before choosing
//! HTML5 `<video>` vs the `stream://` transcode fallback (02-006). The probe
//! never blocks startup (functional spec) — it lives entirely behind a
//! frontend-initiated command.

use serde::{Deserialize, Serialize};

/// Per-codec booleans from the WebView probe. Missing fields fail closed:
/// an unreported codec is treated as unsupported, so playback falls back to
/// `stream://` rather than a black frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CodecSupport {
    #[serde(default)]
    pub h264: bool,
    #[serde(default)]
    pub hevc: bool,
    #[serde(default)]
    pub vp9: bool,
    #[serde(default)]
    pub av1: bool,
}

impl CodecSupport {
    /// One line listing every codec, for the startup debug log and the
    /// diagnostics surface (functional spec: results visible for debugging).
    pub fn log_line(&self) -> String {
        let yes_no = |b: bool| if b { "yes" } else { "no" };
        format!(
            "codec support: h264={} hevc={} vp9={} av1={}",
            yes_no(self.h264),
            yes_no(self.hevc),
            yes_no(self.vp9),
            yes_no(self.av1),
        )
    }
}
