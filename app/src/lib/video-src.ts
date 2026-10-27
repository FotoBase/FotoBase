// Video playback-path resolution (TASK-02-007, PLAN §2 Video strategy).
// Pure decision logic, no DOM and no backend: given the clip's codec (from
// the ffprobe metadata cached at scan time, TASK-02-003) and the WebView's
// codec-support matrix (02-005), decide between the plain HTML5 `<video>`
// path — the asset-protocol URL of the original file — and the `stream://`
// transcode fallback (02-006). Same fail-closed rule as `playbackPath`
// in codec-probe.ts: an unprobed matrix or an unknown codec name takes the
// fallback, which works everywhere.

import { streamSrc } from "./stream";

/// The clip's playback path plus the `<video src>` to use for it.
export type VideoSrcDecision = {
  path: "html5" | "stream";
  src: string;
};

/// Codec names the support matrix knows about (mirrors `CodecSupport`).
const KNOWN_CODECS = ["h264", "hevc", "vp9", "av1"] as const;

type CodecSupport = { [K in (typeof KNOWN_CODECS)[number]]: boolean };

/// Resolve the `<video src>` for one clip.
///
/// `directSrc` is the asset-protocol URL of the original file (the WebView
/// streams it straight from disk, with range requests — no full-file buffer
/// through a scheme handler). It is only usable on the HTML5 path; when it
/// is unavailable (browser dev, missing backend) the resolver falls back to
/// `stream://`, which works everywhere.
export function resolveVideoSrc(
  fileId: number,
  codec: string,
  support: CodecSupport | null,
  directSrc: string | null,
): VideoSrcDecision {
  const known = (KNOWN_CODECS as readonly string[]).includes(codec);
  if (known && support !== null && support[codec as keyof CodecSupport] && directSrc !== null) {
    return { path: "html5", src: directSrc };
  }
  return { path: "stream", src: streamSrc(fileId) };
}

/// CSS transform for the ffprobe display rotation (TASK-02-003). Applied as
/// a transform on the player stage — no re-encode (task technical spec).
/// Degrees normalize to the nearest of 0/90/180/270; anything else is 0.
export function rotationTransform(deg: number): string {
  const norm = ((Math.round(deg / 90) * 90) % 360 + 360) % 360;
  return norm === 0 ? "" : `rotate(${norm}deg)`;
}
