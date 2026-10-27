// `stream://` URL builder tests (TASK-02-006). The scheme serves the
// on-the-fly transcode fallback (PLAN §2 Video strategy): no media bytes
// cross IPC — the `<video>` element fetches the transcoded stream through
// the custom scheme handler, exactly like `thumb://` for images.

import { describe, expect, it, vi } from "vitest";

import { stopStream, streamSrc } from "./stream";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("streamSrc", () => {
  it("builds the plain scheme URI for a file id", () => {
    expect(streamSrc(42)).toBe("stream://42");
  });

  it("appends the seek offset as ?t=", () => {
    // Seeking restarts the transcode at a keyframe near the seek point.
    expect(streamSrc(42, 12.5)).toBe("stream://42?t=12.5");
    expect(streamSrc(42, 0)).toBe("stream://42?t=0");
  });

  it("clamps degenerate offsets instead of building a broken URI", () => {
    expect(streamSrc(42, -3)).toBe("stream://42?t=0");
    expect(streamSrc(42, Number.NaN)).toBe("stream://42?t=0");
    expect(streamSrc(42, Number.POSITIVE_INFINITY)).toBe("stream://42?t=0");
  });
});

describe("stopStream", () => {
  it("tells the backend to cancel the transcode for the file", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear().mockResolvedValue(undefined);
    await stopStream(42);
    expect(invoke).toHaveBeenCalledWith("stop_stream", { fileId: 42 });
  });

  it("never rejects when the backend is missing", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear().mockRejectedValue("backend said no");
    await expect(stopStream(42)).resolves.toBeUndefined();
  });
});
