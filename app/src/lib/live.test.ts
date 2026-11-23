// TASK-08-017 tests: Live/Motion photo badge rules and press-and-hold
// playback. The backend (`live_clip`) is mocked; jsdom has no media playback,
// so play/pause are stubbed on the prototype.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import LivePhotoPlayer from "./LivePhotoPlayer.svelte";
import { hasLiveBadge } from "./live";
import type { GridItem } from "./grid";

vi.mock("@tauri-apps/api/core", () => {
  const invoke = vi.fn();
  const convertFileSrc = (path: string) => `asset://test${path}`;
  return { invoke, convertFileSrc, __invoke: invoke };
});

import * as tauriCore from "@tauri-apps/api/core";
const invokeMock = (tauriCore as unknown as { __invoke: ReturnType<typeof vi.fn> }).__invoke;

const item = (live_kind?: GridItem["live_kind"]): GridItem => ({
  id: 1,
  filename: "a.jpg",
  hash: null,
  live_kind,
});

describe("live badge rule", () => {
  it("shows for motion and live photos only", () => {
    expect(hasLiveBadge(item("motion"))).toBe(true);
    expect(hasLiveBadge(item("live"))).toBe(true);
    expect(hasLiveBadge(item(null))).toBe(false);
    expect(hasLiveBadge(item(undefined))).toBe(false);
  });
});

const play = vi.fn(() => Promise.resolve());
const pause = vi.fn();

beforeEach(() => {
  invokeMock.mockReset();
  play.mockClear();
  pause.mockClear();
  HTMLMediaElement.prototype.play = play as unknown as () => Promise<void>;
  HTMLMediaElement.prototype.pause = pause;
});

afterEach(() => cleanup());

async function renderPlayer() {
  const result = render(LivePhotoPlayer, { fileId: 7, kind: "live" });
  await vi.waitFor(() => screen.getByTestId("live-hold"));
  flushSync();
  return result;
}

describe("LivePhotoPlayer", () => {
  it("plays the clip while held and returns to the still on release", async () => {
    invokeMock.mockResolvedValue({ path: "/lib/IMG_1.MOV", kind: "live" });
    await renderPlayer();
    expect(invokeMock).toHaveBeenCalledWith("live_clip", { fileId: 7 });

    const video = screen.getByTestId("live-video") as HTMLVideoElement;
    expect(video.src).toBe("asset://test/lib/IMG_1.MOV");
    expect(video.muted).toBe(true);
    expect(video.classList.contains("active")).toBe(false);

    await fireEvent.pointerDown(screen.getByTestId("live-hold"));
    expect(play).toHaveBeenCalledTimes(1);
    expect(video.classList.contains("active")).toBe(true);

    await fireEvent.pointerUp(screen.getByTestId("live-hold"));
    expect(pause).toHaveBeenCalled();
    expect(video.classList.contains("active")).toBe(false);
  });

  it("stops when the pointer leaves the button", async () => {
    invokeMock.mockResolvedValue({ path: "/lib/IMG_1.MOV", kind: "live" });
    await renderPlayer();
    await fireEvent.pointerDown(screen.getByTestId("live-hold"));
    await fireEvent.pointerLeave(screen.getByTestId("live-hold"));
    expect(pause).toHaveBeenCalled();
    expect(screen.getByTestId("live-video").classList.contains("active")).toBe(false);
  });

  it("holds with the keyboard too", async () => {
    invokeMock.mockResolvedValue({ path: "/lib/IMG_1.MOV", kind: "live" });
    await renderPlayer();
    const hold = screen.getByTestId("live-hold");
    await fireEvent.keyDown(hold, { key: "Enter" });
    expect(play).toHaveBeenCalledTimes(1);
    await fireEvent.keyUp(hold, { key: "Enter" });
    expect(pause).toHaveBeenCalled();
  });

  it("renders nothing when the photo has no clip or the backend is down", async () => {
    invokeMock.mockResolvedValue(null);
    render(LivePhotoPlayer, { fileId: 7, kind: "motion" });
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalled());
    flushSync();
    expect(screen.queryByTestId("live-hold")).toBeNull();

    cleanup();
    invokeMock.mockRejectedValue(new Error("no backend"));
    render(LivePhotoPlayer, { fileId: 8, kind: "motion" });
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(2));
    flushSync();
    expect(screen.queryByTestId("live-hold")).toBeNull();
  });

  it("falls back to the still when the clip cannot be decoded", async () => {
    invokeMock.mockResolvedValue({ path: "/lib/IMG_1.MOV", kind: "live" });
    await renderPlayer();
    await fireEvent.error(screen.getByTestId("live-video"));
    flushSync();
    expect(screen.queryByTestId("live-hold")).toBeNull();
  });
});
