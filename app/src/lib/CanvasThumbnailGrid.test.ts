// TASK-01-014 component tests: single-canvas grid renderer. jsdom has no
// canvas, so the 2d context is a recording stub; image decoding is injected
// (`loadImage` prop) because jsdom never loads images. The behavioral contract
// mirrors ThumbnailGrid.test.ts — zoom, selection, context-menu coordinates,
// bounded work per frame, visible-hash reporting — on top of the SHARED store.

import { flushSync } from "svelte";
import { cleanup, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import CanvasThumbnailGrid from "./CanvasThumbnailGrid.svelte";
import ThumbnailGrid from "./ThumbnailGrid.svelte";
import { gridState, resetGridState } from "./grid-state.svelte.js";
import type { GridItem } from "./grid";

const HASH = (n: number) => n.toString(16).padStart(32, "0");

function makeItems(count: number): GridItem[] {
  return Array.from({ length: count }, (_, i) => ({
    id: i,
    filename: `photo-${i}.jpg`,
    hash: HASH(i),
  }));
}

type CtxCall = { method: string; args: unknown[] };

let calls: CtxCall[] = [];

/// drawImage calls of the LAST painted frame: draw() starts with a
/// clearRect, so everything after the last one belongs to one frame.
function lastFrameDraws(): CtxCall[] {
  let start = 0;
  calls.forEach((c, i) => {
    if (c.method === "clearRect") start = i;
  });
  return calls.slice(start).filter((c) => c.method === "drawImage");
}

beforeEach(() => {
  resetGridState();
  calls = [];
  // Recording 2d context stub: every draw call lands in `calls`.
  const ctx: Record<string, unknown> = {
    fillStyle: "",
    strokeStyle: "",
    lineWidth: 0,
    font: "",
  };
  for (const method of [
    "clearRect",
    "fillRect",
    "strokeRect",
    "drawImage",
    "fillText",
    "beginPath",
    "save",
    "restore",
    "setTransform",
  ]) {
    ctx[method] = (...args: unknown[]) => calls.push({ method, args });
  }
  HTMLCanvasElement.prototype.getContext = vi
    .fn()
    .mockReturnValue(ctx as unknown as CanvasRenderingContext2D);
});

afterEach(() => cleanup());

// Fake decoder: resolves synchronously-ish with a stand-in image that
// remembers which URI it stands for (so tests can check what was fetched).
function fakeLoadImage(uri: string): Promise<{ src: string; width: number; height: number }> {
  return Promise.resolve({ src: uri, width: 160, height: 160 });
}

async function settle(): Promise<void> {
  flushSync();
  await new Promise((r) => setTimeout(r, 0));
  flushSync();
}

function scrollBy(delta: number): void {
  const el = screen.getByTestId("canvas-grid-scroll");
  el.scrollTop += delta;
  el.dispatchEvent(new Event("scroll"));
  flushSync();
  flushSync();
}

describe("CanvasThumbnailGrid", () => {
  it("draws the visible cells onto ONE canvas, bounded under scroll", async () => {
    render(CanvasThumbnailGrid, {
      items: makeItems(1000),
      loadImage: fakeLoadImage,
    });
    await settle();

    expect(document.querySelector('[data-testid="grid-canvas"]')).not.toBeNull();
    // Thumbnails actually reach drawImage once the injected loader resolves.
    await vi.waitFor(() => {
      const drawn = lastFrameDraws();
      expect(drawn.length).toBeGreaterThan(0);
      // 800×600 viewport at 168px rows fits ~4 rows (+2 overscan) — far
      // fewer than all 1000 items: the canvas draws per frame, not once.
      expect(drawn.length).toBeLessThan(100);
    });

    // Fast scroll through the whole list stays bounded (per frame). The
    // first paint at the far end is placeholders (decode pending) — wait
    // for the loads to land, then the per-frame bound must still hold.
    for (let i = 0; i < 20; i++) scrollBy(4000);
    await vi.waitFor(() => {
      const drawn = lastFrameDraws();
      expect(drawn.length).toBeGreaterThan(0);
      expect(drawn.length).toBeLessThan(100);
    });
  });

  it("fills a placeholder cell for files without a hash yet", async () => {
    const items = makeItems(20);
    items[0] = { ...items[0], hash: null };
    render(CanvasThumbnailGrid, { items, loadImage: fakeLoadImage });
    await settle();

    await vi.waitFor(() => {
      // A fillRect (placeholder paint) happened, and no drawImage carries a
      // payload for the unhashed file.
      expect(calls.some((c) => c.method === "fillRect")).toBe(true);
    });
    const drawnSrcs = calls
      .filter((c) => c.method === "drawImage")
      .map((c) => (c.args[0] as { src: string }).src);
    expect(drawnSrcs).not.toContain("");
  });

  it("zoom slider shares state with the DOM grid and redraws without refetching", async () => {
    render(ThumbnailGrid, { items: makeItems(500) });
    render(CanvasThumbnailGrid, { items: makeItems(500), loadImage: fakeLoadImage });
    await settle();

    const urisBefore = new Set(
      calls.filter((c) => c.method === "drawImage").map((c) => (c.args[0] as { src: string }).src),
    );
    expect(urisBefore.size).toBeGreaterThan(0);

    // Turn the DOM grid's zoom slider: the canvas grid's slider must follow
    // (one shared store), and the canvas must repaint at the new size.
    const domSlider = screen.getAllByTestId("zoom-slider")[0] as HTMLInputElement;
    domSlider.value = "320";
    domSlider.dispatchEvent(new Event("input"));
    flushSync();
    flushSync();

    const canvasSlider = screen.getAllByTestId("zoom-slider")[1] as HTMLInputElement;
    expect(canvasSlider.value).toBe("320");

    // Zoom must not re-fetch: every URI drawn after the zoom was already
    // drawn before (the cache already holds the thumbnails).
    await settle();
    const urisAfter = calls
      .filter((c) => c.method === "drawImage")
      .map((c) => (c.args[0] as { src: string }).src);
    expect(urisAfter.length).toBeGreaterThan(0);
    for (const uri of urisAfter) expect(urisBefore.has(uri)).toBe(true);
  });

  it("click selects a cell into the shared store (toggle off on second click)", async () => {
    render(CanvasThumbnailGrid, { items: makeItems(100), loadImage: fakeLoadImage });
    await settle();

    const canvas = screen.getByTestId("grid-canvas");
    const click = (x: number, y: number): void => {
      const ev = new MouseEvent("click", { bubbles: true });
      Object.defineProperty(ev, "offsetX", { value: x });
      Object.defineProperty(ev, "offsetY", { value: y });
      canvas.dispatchEvent(ev);
      flushSync();
    };

    click(10, 10); // first cell
    expect(gridState.selection.has(0)).toBe(true);
    expect(gridState.selection.size).toBe(1);

    click(200, 10); // second cell (col 1)
    expect(gridState.selection.has(1)).toBe(true);
    expect(gridState.selection.size).toBe(2);

    click(10, 10); // toggle the first off again
    expect(gridState.selection.has(0)).toBe(false);
    expect(gridState.selection.size).toBe(1);
  });

  it("context menu reports the item and the event coordinates", async () => {
    const oncontextmenu = vi.fn();
    render(CanvasThumbnailGrid, {
      items: makeItems(100),
      loadImage: fakeLoadImage,
      oncontextmenu,
    });
    await settle();

    const canvas = screen.getByTestId("grid-canvas");
    const ev = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    Object.defineProperty(ev, "offsetX", { value: 10 });
    Object.defineProperty(ev, "offsetY", { value: 10 });
    Object.defineProperty(ev, "clientX", { value: 110 });
    Object.defineProperty(ev, "clientY", { value: 110 });
    canvas.dispatchEvent(ev);
    flushSync();

    // Menu coordinates are the viewport-relative clientX/Y (where a native
    // context menu would open); the item comes from the hit test.
    expect(oncontextmenu).toHaveBeenCalledWith(
      expect.objectContaining({ x: 110, y: 110, item: expect.objectContaining({ id: 0 }) }),
    );
  });

  it("reports the visible hashes, and re-reports when the scroll range moves", async () => {
    const onvisible = vi.fn();
    render(CanvasThumbnailGrid, {
      items: makeItems(1000),
      loadImage: fakeLoadImage,
      onvisible,
    });
    await new Promise((r) => setTimeout(r, 150));

    expect(onvisible).toHaveBeenCalled();
    const first = onvisible.mock.calls.at(-1)![0] as string[];
    expect(first.length).toBeGreaterThan(0);
    expect(first[0]).toBe(HASH(0));

    scrollBy(4000);
    await new Promise((r) => setTimeout(r, 150));
    const later = onvisible.mock.calls.at(-1)![0] as string[];
    expect(later[0]).not.toBe(HASH(0));
  });
});
