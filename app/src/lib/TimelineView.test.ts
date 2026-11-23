// TASK-08-018 tests: the full-screen timeline component. jsdom has no
// canvas, so the 2d context is a recording stub and image decoding is
// injected. The IPC wrapper is mocked; the query is covered by the catalog
// integration tests.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import TimelineView from "./TimelineView.svelte";
import { setLocale } from "./i18n.svelte.js";
import { CELL, layoutTimeline, loadTimeline, type TimelineGroup } from "./timeline";

vi.mock("./timeline", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./timeline")>()),
  loadTimeline: vi.fn(),
}));

const HASH = (n: number) => n.toString(16).padStart(32, "0");
const GROUPS: TimelineGroup[] = [
  { date: "2022-01-15", count: 2, first_id: 11, hash: HASH(1) },
  { date: "2023-07-01", count: 1, first_id: 22, hash: HASH(2) },
  { date: "2024-03-09", count: 5, first_id: 33, hash: HASH(3) },
];

let calls: { method: string; args: unknown[] }[] = [];

beforeEach(() => {
  setLocale("en");
  calls = [];
  vi.mocked(loadTimeline).mockReset().mockResolvedValue(GROUPS);
  const ctx: Record<string, unknown> = { fillStyle: "", strokeStyle: "", lineWidth: 0, font: "" };
  for (const method of ["clearRect", "fillRect", "strokeRect", "drawImage", "fillText", "setTransform"]) {
    ctx[method] = (...args: unknown[]) => calls.push({ method, args });
  }
  ctx.measureText = (text: string) => ({ width: text.length * 6 });
  HTMLCanvasElement.prototype.getContext = vi
    .fn()
    .mockReturnValue(ctx as unknown as CanvasRenderingContext2D);
});
afterEach(cleanup);

const fakeLoadImage = (uri: string) => Promise.resolve({ src: uri, width: 160, height: 160 });

async function opened(props: Record<string, unknown> = {}) {
  render(TimelineView, { open: true, loadImage: fakeLoadImage, ...props });
  await waitFor(() => expect(screen.getByTestId("timeline-scroll").dataset.focusedDate).not.toBe(""));
  return screen.getByTestId("timeline-scroll");
}

describe("TimelineView", () => {
  it("renders nothing while closed", () => {
    render(TimelineView, { open: false });
    expect(screen.queryByTestId("timeline")).toBeNull();
    expect(loadTimeline).not.toHaveBeenCalled();
  });

  it("sends the grid view to the query and starts on the newest day", async () => {
    const view = {
      folders: [4],
      album: null,
      query: "beach",
      starred: false,
      videosOnly: false,
      hasFaces: false,
      geotagged: false,
      takenSince: null,
      sort: "path" as const,
    };
    const scroll = await opened({ view });
    expect(loadTimeline).toHaveBeenCalledWith(view);
    expect(scroll.dataset.focusedDate).toBe("2024-03-09");
    expect(screen.getByTestId("timeline-status").textContent).toContain("5 photos");
  });

  it("scrubs with the arrow keys, clamped to the ends", async () => {
    const scroll = await opened();
    await fireEvent.keyDown(scroll, { key: "ArrowLeft" });
    expect(scroll.dataset.focusedDate).toBe("2023-07-01");
    await fireEvent.keyDown(scroll, { key: "Home" });
    expect(scroll.dataset.focusedDate).toBe("2022-01-15");
    await fireEvent.keyDown(scroll, { key: "ArrowLeft" });
    expect(scroll.dataset.focusedDate).toBe("2022-01-15");
    await fireEvent.keyDown(scroll, { key: "End" });
    expect(scroll.dataset.focusedDate).toBe("2024-03-09");
  });

  it("Enter opens the focused day", async () => {
    const onopengroup = vi.fn();
    const scroll = await opened({ onopengroup });
    await fireEvent.keyDown(scroll, { key: "Home" });
    await fireEvent.keyDown(scroll, { key: "Enter" });
    expect(onopengroup).toHaveBeenCalledWith(GROUPS[0]);
  });

  it("a click on a group's cell opens that day; a click in a gap does not", async () => {
    const onopengroup = vi.fn();
    await opened({ onopengroup });
    const layout = layoutTimeline(GROUPS);
    const canvas = screen.getByTestId("timeline-canvas");
    // jsdom MouseEvent has no offsetX: define it the way the browser would.
    const click = (offsetX: number) => {
      const e = new MouseEvent("click", { bubbles: true });
      Object.defineProperty(e, "offsetX", { value: offsetX });
      canvas.dispatchEvent(e);
      flushSync();
    };
    click(layout.xs[1] + CELL / 2);
    expect(onopengroup).toHaveBeenCalledTimes(1);
    expect(onopengroup).toHaveBeenCalledWith(GROUPS[1]);
    click(layout.xs[1] + CELL + 2);
    expect(onopengroup).toHaveBeenCalledTimes(1);
  });

  it("Escape closes it", async () => {
    const scroll = await opened();
    await fireEvent.keyDown(scroll, { key: "Escape" });
    await waitFor(() => expect(screen.queryByTestId("timeline")).toBeNull());
  });

  it("paints only on the canvas: one drawImage per decoded visible group", async () => {
    await opened();
    await waitFor(() => expect(calls.filter((c) => c.method === "drawImage")).toHaveLength(3));
    expect(screen.queryAllByRole("img")).toHaveLength(0);
  });

  it("shows the friendly empty state", async () => {
    vi.mocked(loadTimeline).mockResolvedValue([]);
    render(TimelineView, { open: true, loadImage: fakeLoadImage });
    await waitFor(() => expect(screen.getByTestId("timeline-empty")).toBeTruthy());
  });
});
