// Startup codec-capability probe (TASK-02-005, PLAN §10: "WebKitGTK ships
// without H.264/HEVC on several distros"). The WebView answers `canPlayType`
// for each codec we care about; the matrix is reported to the Rust side once
// at startup and cached per session, so playback decisions are data-driven:
// HTML5 `<video>` is the primary path, `stream://` transcode (02-006) only
// for codecs the WebView cannot decode. Pure mapping/decision logic lives
// here so it is unit-testable without a real WebView.

/// One boolean per codec in the set we care about (task spec).
export type CodecSupport = {
  h264: boolean;
  hevc: boolean;
  vp9: boolean;
  av1: boolean;
};

/// `canPlayType` answers "", "maybe", or "probably". Anything non-empty means
/// the WebView believes it can decode; unknown future answers fail closed.
export function parseCanPlayType(answer: string): boolean {
  return answer === "maybe" || answer === "probably";
}

/// The probe vectors: one MIME string per codec, matching what the task
/// spec lists (H.264, HEVC, VP9, AV1).
const PROBE_TYPES = [
  ["h264", "video/mp4; codecs=avc1.42E01E"],
  ["hevc", "video/mp4; codecs=hvc1.1.6.L93.B0"],
  ["vp9", "video/webm; codecs=vp9"],
  ["av1", "video/mp4; codecs=av01.0.08M.08"],
] as const;

/// Run the probe with an injected `canPlayType` so tests never depend on the
/// host WebView's actual codec set. Pure w.r.t. its input.
export function probeCodecs(
  canPlayType: (type: string) => string,
): CodecSupport {
  const out = { h264: false, hevc: false, vp9: false, av1: false };
  for (const [codec, type] of PROBE_TYPES) {
    try {
      out[codec] = parseCanPlayType(canPlayType(type));
    } catch {
      out[codec] = false; // a throwing WebView must not crash startup
    }
  }
  return out;
}

/// Session cache: the probe runs once at startup (functional spec), not per
/// playback decision. `null` until the first probe.
let cached: CodecSupport | null = null;

/// Probe via the DOM and cache the matrix for the session. The injected
/// overload exists for tests; without one it uses a real `<video>` element.
export function probeAndCache(canPlayType?: (type: string) => string): CodecSupport {
  cached ??= probeCodecs((type) => {
    if (canPlayType) return canPlayType(type);
    const video = document.createElement("video");
    return video.canPlayType(type);
  });
  return cached;
}

/// Primary container per codec, for building the `<video src>` on the HTML5
/// path (H.264/HEVC/AV1 → mp4, VP9 → webm).
export const codecContainers = {
  h264: "mp4",
  hevc: "mp4",
  vp9: "webm",
  av1: "mp4",
} as const;

/// Which playback path a clip takes: plain HTML5 `<video>` when the WebView
/// supports the codec, otherwise the `stream://` transcode fallback
/// (02-006). An unprobed matrix (`null`) fails closed to the fallback.
export function playbackPath(
  codec: keyof CodecSupport,
  support: CodecSupport | null,
): "html5" | "stream" {
  return support !== null && support[codec] ? "html5" : "stream";
}

/// Startup wiring (functional spec: probe runs once, never blocks startup):
/// probe + cache the matrix, report it to the backend, best-effort — a
/// missing or failing backend must never break boot (same policy as the
/// other best-effort IPC calls).
export async function reportCodecSupport(): Promise<void> {
  const support = probeAndCache();
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("report_codec_support", { support });
  } catch {
    // Browser dev / backend down: the frontend matrix is still usable.
  }
}

