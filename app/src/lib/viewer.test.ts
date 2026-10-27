// TASK-01-019 tests: single-photo viewer. Two layers in one file (the
// case-insensitive checkout cannot hold `viewer.test.ts` and
// `Viewer.test.ts` side by side):
//
// 1. Pure viewer math (same pattern as grid.test.ts) — navigation
//    wrap-around, preloading, preview URI rules.
// 2. The component — filmstrip, keyboard control, timed slideshow.
//    Fullscreen goes through the Tauri window API (mocked here — jsdom has
//    none); slideshow timing uses fake timers.

import { flushSync } from "svelte";
import { cleanup, render, screen, fireEvent } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Viewer from "./Viewer.svelte";
import { probeAndCache } from "./codec-probe";
import { nextIndex, preloadUris, previewUri, prevIndex } from "./viewer";
import type { GridItem } from "./grid";

// --- Pure logic -------------------------------------------------------------

const HASH = (n: number) => n.toString(16).padStart(32, "0");

describe("viewer math", () => {
  it("advance and go back inside the list", () => {
    expect(nextIndex(0, 3)).toBe(1);
    expect(prevIndex(1, 3)).toBe(0);
  });

  it("wrap around at both ends", () => {
    expect(nextIndex(2, 3)).toBe(0);
    expect(prevIndex(0, 3)).toBe(2);
  });

  it("degenerate to index 0 on an empty list", () => {
    expect(nextIndex(0, 0)).toBe(0);
    expect(prevIndex(0, 0)).toBe(0);
  });

  it("builds a preview:// URI from a well-formed cache key", () => {
    expect(previewUri(HASH(1))).toBe(`preview://${HASH(1)}`);
  });

  it("rejects null and malformed hashes (placeholder rule, not an error)", () => {
    expect(previewUri(null)).toBeNull();
    expect(previewUri("../etc/passwd")).toBeNull();
    expect(previewUri("FFF")).toBeNull();
  });

  it("preloads the previous and next URIs around the current index", () => {
    const uris = [0, 1, 2].map((n) => `preview://${HASH(n)}`);
    expect(preloadUris(uris, 1)).toEqual([uris[0], uris[2]]);
    expect(preloadUris(uris, 0)).toEqual([uris[2], uris[1]]);
    expect(preloadUris(uris, 2)).toEqual([uris[1], uris[0]]);
  });

  it("dedupes a two-photo preload and skips null hashes", () => {
    const two = [`preview://${HASH(0)}`, `preview://${HASH(1)}`];
    expect(preloadUris(two, 0)).toEqual([two[1]]);
    expect(preloadUris([null, two[1]], 0)).toEqual([two[1]]);
  });
});

// --- Component --------------------------------------------------------------

function makeItems(count: number): GridItem[] {
  return Array.from({ length: count }, (_, i) => ({
    id: i,
    filename: `photo-${i}.jpg`,
    hash: HASH(i),
  }));
}

vi.mock("@tauri-apps/api/window", () => {
  const setFullscreen = vi.fn();
  return {
    getCurrentWindow: () => ({ setFullscreen }),
    // Exposed for assertions (vitest keeps the factory module cached).
    __setFullscreen: setFullscreen,
  };
});

// TASK-02-007: the viewer asks the backend for the player payload of the
// current item (`video_info`) and builds the direct-play URL with
// `convertFileSrc`. Mocked here: `invoke` answers from a map, the asset
// URL is a plain marker string.
vi.mock("@tauri-apps/api/core", () => {
  const invoke = vi.fn();
  const convertFileSrc = (path: string) => `asset://test${path}`;
  return {
    invoke,
    convertFileSrc,
    __invoke: invoke,
    __convertFileSrc: convertFileSrc,
  };
});

import * as tauriCore from "@tauri-apps/api/core";
const invokeMock = (tauriCore as unknown as { __invoke: ReturnType<typeof vi.fn> }).__invoke;
const convertFileSrcMock = (
  tauriCore as unknown as { __convertFileSrc: (p: string) => string }
).__convertFileSrc;

const VIDEO_META = {
  duration_secs: 12.5,
  codec: "h264",
  profile: null,
  rotation_deg: 90,
};

function videoItem(id: number): GridItem {
  return { id, filename: `clip-${id}.mov`, hash: HASH(id), media_type: "video" };
}

async function renderVideoViewer(): Promise<{ container: HTMLElement }> {
  // Seed the codec cache: this WebView reports H.264 support, so the
  // resolver takes the html5 path.
  probeAndCache((type) => (type.includes("avc1") ? "probably" : ""));
  invokeMock.mockImplementation((_cmd: string, args: { fileId: number }) =>
    Promise.resolve(args.fileId === 1 ? { path: "/lib/clip.mov", meta: VIDEO_META } : null),
  );
  const result = render(Viewer, {
    items: [makeItems(3)[0], videoItem(1), makeItems(3)[2]],
    startIndex: 1,
  });
  // The video_info round-trip resolves on a microtask.
  await new Promise((r) => setTimeout(r, 0));
  flushSync();
  return result;
}

// The mock factory's exports are what the component imports; grab the spy
// through the mocked module for assertions.
import * as tauriWindow from "@tauri-apps/api/window";
const setFullscreenMock = (
  tauriWindow as unknown as { __setFullscreen: ReturnType<typeof vi.fn> }
).__setFullscreen;

function pressKey(key: string): void {
  fireEvent.keyDown(window, { key });
  flushSync();
}

function renderViewer(items: GridItem[], props: Record<string, unknown> = {}) {
  const result = render(Viewer, { items, ...props });
  flushSync();
  return result;
}

function mainSrc(): string {
  return (screen.getByTestId("viewer-main") as HTMLImageElement).src;
}

beforeEach(() => {
  setFullscreenMock.mockClear();
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(null);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("Viewer", () => {
  it("shows the current photo full-size via preview:// and a filmstrip of the selection", () => {
    renderViewer(makeItems(3), { startIndex: 1 });

    expect(mainSrc()).toContain(HASH(1));

    const strip = document.querySelectorAll('[data-testid="filmstrip-thumb"]');
    expect(strip.length).toBe(3);
    // Every strip entry carries its thumb:// URI (thumbnails, not previews).
    expect(strip[0].getAttribute("src")).toBe(`thumb://${HASH(0)}`);
    // The current one is marked.
    expect(strip[1].classList.contains("current")).toBe(true);
  });

  it("arrow keys move the selection with wrap-around", () => {
    renderViewer(makeItems(3), { startIndex: 2 });

    pressKey("ArrowRight");
    expect(mainSrc()).toContain(HASH(0));

    pressKey("ArrowLeft");
    pressKey("ArrowLeft");
    expect(mainSrc()).toContain(HASH(1));
  });

  it("Esc exits the viewer", () => {
    const onclose = vi.fn();
    renderViewer(makeItems(3), { onclose });

    pressKey("Escape");
    expect(onclose).toHaveBeenCalledOnce();
  });

  it("clicking a filmstrip thumb jumps to that photo", () => {
    renderViewer(makeItems(3));

    fireEvent.click(screen.getAllByTestId("filmstrip-thumb")[2]);
    flushSync();
    expect(mainSrc()).toContain(HASH(2));
  });

  it("F toggles fullscreen through the Tauri window API", async () => {
    renderViewer(makeItems(3));

    // The toggle is async (dynamic import of the Tauri API): let the
    // microtasks land before asserting.
    pressKey("f");
    await vi.waitFor(() => expect(setFullscreenMock).toHaveBeenLastCalledWith(true));
    pressKey("f");
    await vi.waitFor(() => expect(setFullscreenMock).toHaveBeenLastCalledWith(false));
  });

  it("space starts the slideshow; it advances through all photos in order", () => {
    vi.useFakeTimers();
    renderViewer(makeItems(3), { slideshowMs: 1000 });

    pressKey(" ");
    expect(mainSrc()).toContain(HASH(0));

    vi.advanceTimersByTime(1000);
    flushSync();
    expect(mainSrc()).toContain(HASH(1));

    vi.advanceTimersByTime(1000);
    flushSync();
    expect(mainSrc()).toContain(HASH(2));

    vi.advanceTimersByTime(1000);
    flushSync();
    // Wraps back to the first photo.
    expect(mainSrc()).toContain(HASH(0));
  });

  it("space again stops the slideshow", () => {
    vi.useFakeTimers();
    renderViewer(makeItems(3), { slideshowMs: 1000 });

    pressKey(" ");
    pressKey(" ");
    vi.advanceTimersByTime(5000);
    flushSync();
    expect(mainSrc()).toContain(HASH(0));
  });

  it("arrow keys pause a running slideshow", () => {
    vi.useFakeTimers();
    renderViewer(makeItems(3), { slideshowMs: 1000 });

    pressKey(" ");
    vi.advanceTimersByTime(1000);
    flushSync();
    expect(mainSrc()).toContain(HASH(1));

    // Interaction pauses: the timer does not advance the photo anymore.
    pressKey("ArrowRight");
    vi.advanceTimersByTime(5000);
    flushSync();
    expect(mainSrc()).toContain(HASH(2));
  });

  it("starts paused by default and can be opened with the slideshow running", () => {
    vi.useFakeTimers();
    renderViewer(makeItems(3), { slideshowMs: 1000, startSlideshow: true });

    vi.advanceTimersByTime(1000);
    flushSync();
    expect(mainSrc()).toContain(HASH(1));
  });

  it("preloads the neighbor photos when the current photo changes", () => {
    const loads: string[] = [];
    const origImage = window.Image;
    class FakeImage {
      set src(v: string) {
        loads.push(v);
      }
    }
    (window as unknown as { Image: unknown }).Image = FakeImage;

    try {
      renderViewer(makeItems(3), { startIndex: 0 });
      // Neighbors of index 0: photo 2 (wrap) and photo 1.
      expect(loads).toEqual([`preview://${HASH(2)}`, `preview://${HASH(1)}`]);
    } finally {
      (window as unknown as { Image: unknown }).Image = origImage;
    }
  });

  // --- Video (TASK-02-007) ---------------------------------------------------

  it("renders the video player for a video item instead of the image", async () => {
    const { container } = await renderVideoViewer();

    expect(container.querySelector("[data-testid='video-player']")).not.toBeNull();
    expect(container.querySelector("[data-testid='viewer-main']")).toBeNull();
    // Direct-play URL through the asset protocol for the supported codec.
    const video = container.querySelector("video");
    expect(video?.getAttribute("src")).toBe(convertFileSrcMock("/lib/clip.mov"));
    // Rotation from ffprobe metadata, applied as a CSS transform.
    expect(
      container
        .querySelector("[data-testid='video-stage']")
        ?.getAttribute("style"),
    ).toContain("rotate(90deg)");
  });

  it("space/arrow keys go to the player while a video is open (viewer navigation pauses)", async () => {
    const { container } = await renderVideoViewer();
    const video = container.querySelector("video")!;
    video.play = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(video, "paused", { value: true, configurable: true });

    pressKey(" ");
    expect(video.play).toHaveBeenCalled();
    // No navigation happened: the main is still the player for item 1.
    expect(container.querySelector("[data-testid='video-player']")).not.toBeNull();
  });

  it("closing the viewer stops playback and cancels the transcode", async () => {
    const { container } = await renderVideoViewer();
    const video = container.querySelector("video")!;
    video.pause = vi.fn();
    // Simulate the App unmounting the viewer on close.
    cleanup();
    await new Promise((r) => setTimeout(r, 0));
    expect(video.pause).toHaveBeenCalled();
    expect(invokeMock).toHaveBeenCalledWith("stop_stream", { fileId: 1 });
  });

  it("preloads skip video neighbors (no full-file preview fetch)", async () => {
    const loads: string[] = [];
    const origImage = window.Image;
    class FakeImage {
      set src(v: string) {
        loads.push(v);
      }
    }
    (window as unknown as { Image: unknown }).Image = FakeImage;
    invokeMock.mockResolvedValue(null);

    try {
      const items = [makeItems(3)[0], videoItem(1), makeItems(3)[2]];
      render(Viewer, { items, startIndex: 0 });
      flushSync();
      // The video neighbor's preview:// URI is never requested; only the
      // wrap-around image neighbor (index 2) loads.
      expect(loads).toEqual([`preview://${HASH(2)}`]);
    } finally {
      (window as unknown as { Image: unknown }).Image = origImage;
    }
  });
});
