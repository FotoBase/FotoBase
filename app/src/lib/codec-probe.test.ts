import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  type CodecSupport,
  codecContainers,
  parseCanPlayType,
  playbackPath,
  probeAndCache,
  probeCodecs,
  reportCodecSupport,
} from "./codec-probe";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("parseCanPlayType", () => {
  // WebView reality: `canPlayType` answers "", "maybe", or "probably" —
  // anything non-empty means the WebView believes it can decode.
  it("maps empty string to unsupported", () => {
    expect(parseCanPlayType("")).toBe(false);
  });

  it("maps 'maybe' and 'probably' to supported", () => {
    expect(parseCanPlayType("maybe")).toBe(true);
    expect(parseCanPlayType("probably")).toBe(true);
  });

  it("treats unknown future answers as unsupported (fail closed)", () => {
    expect(parseCanPlayType("??")).toBe(false);
  });
});

// The probe injects its own canPlayType, so jsdom's always-blank answer
// never leaks into the tests; results come from the injected mapping.
const canPlay = (type: string) =>
  ({
    "video/mp4; codecs=avc1.42E01E": "probably",
    "video/mp4; codecs=hvc1.1.6.L93.B0": "",
    "video/webm; codecs=vp9": "maybe",
    "video/mp4; codecs=av01.0.08M.08": "probably",
  })[type] ?? "";

describe("probeCodecs", () => {
  it("builds a per-codec boolean matrix", () => {
    expect(probeCodecs(canPlay)).toEqual({
      h264: true,
      hevc: false,
      vp9: true,
      av1: true,
    });
  });

  it("is cached per session — second call returns the first result", () => {
    expect(probeAndCache(canPlay)).toBe(probeAndCache(canPlay));
  });

  it("never throws on a degenerate probe function", () => {
    expect(probeCodecs(() => "")).toEqual({
      h264: false,
      hevc: false,
      vp9: false,
      av1: false,
    });
  });
});

describe("playbackPath", () => {
  const supported: CodecSupport = { h264: true, hevc: false, vp9: true, av1: false };

  it("routes a supported codec to plain HTML5 <video>", () => {
    expect(playbackPath("h264", supported)).toBe("html5");
  });

  it("routes an unsupported codec to the transcode fallback (02-006)", () => {
    expect(playbackPath("hevc", supported)).toBe("stream");
  });

  it("routes an unprobed codec to the safe fallback", () => {
    expect(playbackPath("hevc", null)).toBe("stream");
  });
});

describe("reportCodecSupport", () => {
  it("probes, caches, and reports the matrix to the backend", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear().mockResolvedValue(undefined);
    probeAndCache(canPlay); // prime the cache with the fixture matrix
    await reportCodecSupport();
    expect(invoke).toHaveBeenCalledWith("report_codec_support", {
      support: { h264: true, hevc: false, vp9: true, av1: true },
    });
  });

  it("never rejects when the backend is missing", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear().mockRejectedValue("backend said no");
    await expect(reportCodecSupport()).resolves.toBeUndefined();
  });
});

describe("codecContainers", () => {
  // Needed to build the <video src> for the HTML5 path (codec → container).
  it("maps codecs to their primary containers", () => {
    expect(codecContainers.h264).toBe("mp4");
    expect(codecContainers.hevc).toBe("mp4");
    expect(codecContainers.vp9).toBe("webm");
    expect(codecContainers.av1).toBe("mp4");
  });
});
