// TASK-02-007 component tests: the video player. The `<video>` element gets
// its `src` from the resolver (html5 asset URL vs `stream://`), shows
// play/pause + scrub + catalog duration, applies rotation as a CSS
// transform, and cleans up on destroy (pause, clear src, cancel transcode).
// Media playback is jsdom-stubbed: the element's methods are spies, the
// resolver input is injected, so the tests assert decisions + teardown, not
// real decoding.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import VideoPlayer from "./VideoPlayer.svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const PROPS = {
  fileId: 7,
  codec: "h264",
  rotationDeg: 0,
  durationSecs: 12.5,
  directSrc: "asset://clip.mp4",
  support: { h264: true, hevc: false, vp9: false, av1: false },
};

describe("VideoPlayer", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("renders a video element pointing at the resolved src", () => {
    const { container } = render(VideoPlayer, { props: PROPS });
    const video = container.querySelector("video");
    expect(video).not.toBeNull();
    expect(video?.getAttribute("src")).toBe("asset://clip.mp4");
  });

  it("uses stream:// when the codec is unsupported", () => {
    const { container } = render(VideoPlayer, {
      props: { ...PROPS, codec: "hevc" },
    });
    expect(container.querySelector("video")?.getAttribute("src")).toBe(
      "stream://7",
    );
  });

  it("applies rotation as a CSS transform", () => {
    const { container } = render(VideoPlayer, {
      props: { ...PROPS, rotationDeg: 90 },
    });
    const stage = container.querySelector("[data-testid='video-stage']");
    expect(stage?.getAttribute("style")).toContain("rotate(90deg)");
  });

  it("shows the catalog duration", () => {
    render(VideoPlayer, { props: PROPS });
    expect(screen.getByTestId("video-duration").textContent).toContain("0:12");
  });

  it("play/pause button toggles element playback", async () => {
    const { container } = render(VideoPlayer, { props: PROPS });
    const video = container.querySelector("video")!;
    video.play = vi.fn().mockResolvedValue(undefined);
    video.pause = vi.fn();
    Object.defineProperty(video, "paused", { value: true, configurable: true });
    await fireEvent.click(screen.getByTestId("video-play"));
    expect(video.play).toHaveBeenCalled();
    Object.defineProperty(video, "paused", { value: false, configurable: true });
    await fireEvent.click(screen.getByTestId("video-play"));
    expect(video.pause).toHaveBeenCalled();
  });

  it("space toggles playback, arrows seek (viewer-consistent shortcuts)", async () => {
    const { container } = render(VideoPlayer, { props: PROPS });
    const video = container.querySelector("video")!;
    video.play = vi.fn().mockResolvedValue(undefined);
    video.pause = vi.fn();
    Object.defineProperty(video, "currentTime", {
      value: 5,
      writable: true,
    });
    await fireEvent.keyDown(window, { key: " " });
    expect(video.play).toHaveBeenCalled();
    await fireEvent.keyDown(window, { key: "ArrowRight" });
    expect(video.currentTime).toBe(10);
    await fireEvent.keyDown(window, { key: "ArrowLeft" });
    expect(video.currentTime).toBe(5);
  });

  it("stops cleanly on destroy: pause, clear src, cancel transcode", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const { container, unmount } = render(VideoPlayer, {
      props: { ...PROPS, codec: "hevc" },
    });
    const video = container.querySelector("video")!;
    video.pause = vi.fn();
    unmount();
    // stopStream is fire-and-forget through a dynamic import — one tick.
    await new Promise((r) => setTimeout(r, 0));
    expect(video.pause).toHaveBeenCalled();
    expect(video.getAttribute("src")).toBeNull();
    expect(invoke).toHaveBeenCalledWith("stop_stream", { fileId: 7 });
  });

  it("scrub bar seeks the element", async () => {
    const { container } = render(VideoPlayer, {
      props: { ...PROPS, durationSecs: 100 },
    });
    const video = container.querySelector("video")!;
    const scrub = container.querySelector(
      "[data-testid='video-scrub']",
    ) as HTMLInputElement;
    Object.defineProperty(video, "duration", { value: 100, writable: true });
    fireEvent.input(scrub, { target: { value: "50" } });
    flushSync();
    expect(video.currentTime).toBe(50);
  });
});
