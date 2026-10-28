// TASK-03-005 tests: star / 1-5 rating in the viewer. IPC is mocked; the
// component's job is the decision (which value to send) and reflecting the
// backend's answer.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import Viewer from "./Viewer.svelte";
import type { GridItem } from "./grid";
import { ratingForKey, STAR, toggleStar } from "./rating";

vi.mock("@tauri-apps/api/core", () => {
  const invoke = vi.fn();
  return { invoke, convertFileSrc: (p: string) => `asset://test${p}`, __invoke: invoke };
});
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ setFullscreen: vi.fn() }),
}));

import * as tauriCore from "@tauri-apps/api/core";
const invokeMock = (tauriCore as unknown as { __invoke: ReturnType<typeof vi.fn> }).__invoke;

const HASH = (n: number) => n.toString(16).padStart(32, "0");

function items(ratings: number[]): GridItem[] {
  return ratings.map((rating, i) => ({
    id: i + 1,
    filename: `p${i}.jpg`,
    hash: HASH(i),
    rating,
  }));
}

const settle = async (): Promise<void> => {
  await new Promise((r) => setTimeout(r, 0));
  flushSync();
};

beforeEach(() => {
  invokeMock.mockReset();
  // video_info -> null (image); set_rating echoes the requested rating.
  invokeMock.mockImplementation((cmd: string, args: { rating?: number }) =>
    Promise.resolve(cmd === "set_rating" ? (args.rating ?? 0) : null),
  );
});

afterEach(cleanup);

describe("rating helpers", () => {
  it("star toggles between 5 and 0", () => {
    expect(toggleStar(0)).toBe(STAR);
    expect(toggleStar(5)).toBe(0);
    // A 3 from another tool counts as rated: toggling clears it.
    expect(toggleStar(3)).toBe(0);
  });

  it("digit keys only rate when ratings are enabled", () => {
    expect(ratingForKey("3", true)).toBe(3);
    expect(ratingForKey("0", true)).toBe(0);
    expect(ratingForKey("3", false)).toBeNull();
    expect(ratingForKey("6", true)).toBeNull();
    expect(ratingForKey("a", true)).toBeNull();
  });
});

describe("Viewer star (star-only mode)", () => {
  it("clicking the star writes rating 5 and shows it pressed", async () => {
    const onrate = vi.fn();
    render(Viewer, { items: items([0, 0]), startIndex: 0, onrate });
    const star = screen.getByTestId("star-toggle");
    expect(star.getAttribute("aria-pressed")).toBe("false");

    await fireEvent.click(star);
    await settle();

    expect(invokeMock).toHaveBeenCalledWith("set_rating", { fileId: 1, rating: 5 });
    expect(screen.getByTestId("star-toggle").getAttribute("aria-pressed")).toBe("true");
    expect(onrate).toHaveBeenCalledWith(1, 5);
  });

  it("clicking a starred photo's star unstars it (rating 0)", async () => {
    render(Viewer, { items: items([5]), startIndex: 0 });
    const star = screen.getByTestId("star-toggle");
    expect(star.getAttribute("aria-pressed")).toBe("true");

    await fireEvent.click(star);
    await settle();

    expect(invokeMock).toHaveBeenCalledWith("set_rating", { fileId: 1, rating: 0 });
    expect(screen.getByTestId("star-toggle").getAttribute("aria-pressed")).toBe("false");
  });

  it("the S key toggles the star", async () => {
    render(Viewer, { items: items([0]), startIndex: 0 });
    await fireEvent.keyDown(window, { key: "s" });
    await settle();
    expect(invokeMock).toHaveBeenCalledWith("set_rating", { fileId: 1, rating: 5 });
  });

  it("digit keys do nothing while ratings are disabled", async () => {
    render(Viewer, { items: items([0]), startIndex: 0 });
    await fireEvent.keyDown(window, { key: "3" });
    await settle();
    expect(invokeMock).not.toHaveBeenCalledWith("set_rating", expect.anything());
  });

  it("a failed write leaves the star unchanged and reports the error", async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "set_rating" ? Promise.reject("sidecar write failed") : Promise.resolve(null),
    );
    render(Viewer, { items: items([0]), startIndex: 0 });

    await fireEvent.click(screen.getByTestId("star-toggle"));
    await settle();

    expect(screen.getByTestId("star-toggle").getAttribute("aria-pressed")).toBe("false");
    expect(screen.getByTestId("rating-error").textContent).toContain("sidecar write failed");
  });
});

describe("Viewer ratings (opt-in 1-5)", () => {
  it("shows five rating buttons and sends the clicked value", async () => {
    render(Viewer, { items: items([0]), startIndex: 0, ratingsEnabled: true });
    expect(screen.queryByTestId("star-toggle")).toBeNull();

    await fireEvent.click(screen.getByTestId("rating-3"));
    await settle();

    expect(invokeMock).toHaveBeenCalledWith("set_rating", { fileId: 1, rating: 3 });
    expect(screen.getByTestId("rating-3").getAttribute("aria-pressed")).toBe("true");
  });

  it("clicking the current rating again clears it", async () => {
    render(Viewer, { items: items([4]), startIndex: 0, ratingsEnabled: true });
    await fireEvent.click(screen.getByTestId("rating-4"));
    await settle();
    expect(invokeMock).toHaveBeenCalledWith("set_rating", { fileId: 1, rating: 0 });
  });

  it("digit keys rate the current photo", async () => {
    render(Viewer, { items: items([0]), startIndex: 0, ratingsEnabled: true });
    await fireEvent.keyDown(window, { key: "2" });
    await settle();
    expect(invokeMock).toHaveBeenCalledWith("set_rating", { fileId: 1, rating: 2 });
  });
});
