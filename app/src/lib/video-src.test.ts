// Video src resolution tests (TASK-02-007, PLAN §2 Video strategy). The
// resolver picks the playback path for a clip: plain HTML5 `<video>` (the
// asset-protocol URL of the original file) when the codec probe (02-005)
// says the WebView can decode it, `stream://` (02-006) otherwise. Pure
// logic — no DOM, no backend.

import { describe, expect, it } from "vitest";

import { rotationTransform, resolveVideoSrc } from "./video-src";

const FULL = { h264: true, hevc: true, vp9: true, av1: true };
const ONLY_H264 = { h264: true, hevc: false, vp9: false, av1: false };

describe("resolveVideoSrc", () => {
  it("uses the direct asset URL when the codec is supported", () => {
    expect(resolveVideoSrc(7, "h264", FULL, "asset://orig")).toEqual({
      path: "html5",
      src: "asset://orig",
    });
  });

  it("falls back to stream:// for unsupported codecs", () => {
    expect(resolveVideoSrc(7, "hevc", ONLY_H264, "asset://orig")).toEqual({
      path: "stream",
      src: "stream://7",
    });
    expect(resolveVideoSrc(7, "vp9", ONLY_H264, "asset://orig").src).toBe(
      "stream://7",
    );
  });

  it("fails closed to stream:// when the probe has not run", () => {
    expect(resolveVideoSrc(7, "h264", null, "asset://orig")).toEqual({
      path: "stream",
      src: "stream://7",
    });
  });

  it("fails closed to stream:// for unknown codec names", () => {
    expect(resolveVideoSrc(7, "mpeg4", FULL, "asset://orig").path).toBe(
      "stream",
    );
    expect(resolveVideoSrc(7, "", FULL, "asset://orig").path).toBe("stream");
  });

  it("falls back to stream:// when no direct URL is available", () => {
    // Browser dev / missing backend: the asset URL may be unavailable; the
    // transcode scheme is still there.
    expect(resolveVideoSrc(7, "h264", FULL, null)).toEqual({
      path: "stream",
      src: "stream://7",
    });
  });
});

describe("rotationTransform", () => {
  // Rotation from ffprobe metadata (02-003) applied as a CSS transform —
  // no re-encode (task technical spec). Degrees normalize to the nearest
  // of 0/90/180/270.
  it("is empty for 0 degrees", () => {
    expect(rotationTransform(0)).toBe("");
  });

  it("maps the quarter turns", () => {
    expect(rotationTransform(90)).toBe("rotate(90deg)");
    expect(rotationTransform(180)).toBe("rotate(180deg)");
    expect(rotationTransform(270)).toBe("rotate(270deg)");
  });

  it("normalizes out-of-range and negative values", () => {
    expect(rotationTransform(360)).toBe("");
    expect(rotationTransform(-90)).toBe("rotate(270deg)");
    expect(rotationTransform(450)).toBe("rotate(90deg)");
  });
});
