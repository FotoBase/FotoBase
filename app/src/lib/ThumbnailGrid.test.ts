// TASK-01-013 component tests: virtualized DOM grid, bounded DOM under fast
// scroll, live zoom slider, visible-range reporting.
//
// jsdom has no layout, so the scroll element's dimensions come from the
// test-setup prototypes (800×600 unless a test overrides `__clientWidth` /
// `__clientHeight`). Rows are fixed-size (no dynamic measurement), which is
// also what keeps the grid cheap on 50k items.

import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { flushSync } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import ThumbnailGrid from "./ThumbnailGrid.svelte";
import type { GridItem } from "./grid";
import { gridState, resetGridState } from "./grid-state.svelte.js";

const HASH = (n: number) => n.toString(16).padStart(32, "0");

function makeItems(count: number): GridItem[] {
  return Array.from({ length: count }, (_, i) => ({
    id: i,
    filename: `photo-${i}.jpg`,
    hash: HASH(i),
  }));
}

function scrollBy(delta: number): void {
  const el = screen.getByTestId("grid-scroll");
  el.scrollTop += delta;
  el.dispatchEvent(new Event("scroll"));
  flushSync();
  flushSync();
}

afterEach(() => {
  resetGridState();
  cleanup();
});

describe("ThumbnailGrid reveal (tray jump, TASK-03-010)", () => {
  it("scrolls to the requested photo, selects it, and clears the request", () => {
    render(ThumbnailGrid, { items: makeItems(1000) });
    const el = screen.getByTestId("grid-scroll");
    expect(el.scrollTop).toBe(0);

    gridState.revealRequest = { id: 500 };
    flushSync();
    flushSync();

    // 800px wide at 160px cells + 8px gaps → 4 columns; item 500 is in row 125.
    expect(el.scrollTop).toBe(125 * 168);
    expect([...gridState.selection]).toEqual([500]);
    expect(gridState.revealRequest).toBeNull();
  });

  it("waits for the photo to appear in the shown items", () => {
    const { rerender } = render(ThumbnailGrid, { items: makeItems(10) });
    gridState.revealRequest = { id: 500 };
    flushSync();
    expect(gridState.revealRequest).toEqual({ id: 500 });

    void rerender({ items: makeItems(1000) });
    flushSync();
    flushSync();
    expect(gridState.revealRequest).toBeNull();
    expect([...gridState.selection]).toEqual([500]);
  });
});

describe("ThumbnailGrid", () => {
  it("mounts a bounded DOM for 1000 items (only visible rows exist)", () => {
    render(ThumbnailGrid, { items: makeItems(1000) });

    const cells = document.querySelectorAll('[data-testid="grid-cell"]');
    expect(cells.length).toBeGreaterThan(0);
    // 800×600 viewport at 168px rows fits ~4 rows; overscan 2 adds a few.
    // The point of the bound: far fewer than all 1000 items.
    expect(cells.length).toBeLessThan(100);
  });

  it("stays bounded while scrolling fast through the whole list", () => {
    render(ThumbnailGrid, { items: makeItems(1000) });

    for (let i = 0; i < 20; i++) scrollBy(4000);

    const cells = document.querySelectorAll('[data-testid="grid-cell"]');
    expect(cells.length).toBeGreaterThan(0);
    expect(cells.length).toBeLessThan(100);
    // The far end of the list is actually rendered.
    const last = document.querySelector(
      '[data-testid="grid-cell"] img[src="thumb://' + HASH(999) + '"]',
    );
    expect(last).not.toBeNull();
  });

  it("shows the placeholder for files without a hash yet", () => {
    const items = makeItems(20);
    items[0] = { ...items[0], hash: null };
    render(ThumbnailGrid, { items });

    expect(
      document.querySelector('[data-testid="grid-placeholder"]'),
    ).not.toBeNull();
  });

  it("arrow keys move focus and selection (TASK-01-024)", () => {
    render(ThumbnailGrid, { items: makeItems(12) });

    const first = document.querySelector(
      '[data-cell-index="0"]',
    ) as HTMLElement;
    first.focus();
    fireEvent.keyDown(first, { key: "ArrowDown" });
    flushSync();

    // 800px viewport at 160px thumbs = 4 columns; Down from 0 lands on 4.
    const target = document.querySelector(
      '[data-cell-index="4"]',
    ) as HTMLElement;
    expect(target).not.toBeNull();
    expect(document.activeElement).toBe(target);
    expect(gridState.selection.has(4)).toBe(true);
    expect(gridState.selection.has(0)).toBe(false);
  });

  it("arrow navigation never leaves the grid", () => {
    render(ThumbnailGrid, { items: makeItems(6) });

    const first = document.querySelector(
      '[data-cell-index="0"]',
    ) as HTMLElement;
    first.focus();
    fireEvent.keyDown(first, { key: "ArrowLeft" });
    flushSync();

    expect(document.activeElement).toBe(first);
    expect(gridState.selection.size).toBe(0);
  });

  it("zoom slider resizes thumbs live without re-fetching (srcs unchanged)", () => {
    render(ThumbnailGrid, { items: makeItems(500) });

    const srcsBefore = [
      ...document.querySelectorAll('[data-testid="grid-cell"] img'),
    ].map((img) => img.getAttribute("src"));

    const slider = screen.getByTestId("zoom-slider") as HTMLInputElement;
    slider.value = "320";
    slider.dispatchEvent(new Event("input"));
    flushSync();
    flushSync();

    const srcsAfter = [
      ...document.querySelectorAll('[data-testid="grid-cell"] img'),
    ].map((img) => img.getAttribute("src"));
    // Zooming must not re-fetch: every rendered src was already rendered at
    // the old zoom level (fewer cells fit on screen at 320px, so a strict
    // subset — no new content fetched, none dropped that was still visible).
    expect(srcsAfter.length).toBeGreaterThan(0);
    expect(srcsBefore).toEqual(expect.arrayContaining(srcsAfter));

    // The cells now size from the new zoom level.
    const cell = document.querySelector(
      '[data-testid="grid-cell"]',
    ) as HTMLElement;
    expect(cell.style.getPropertyValue("--thumb-size")).toBe("320px");
  });

  it("re-requests a thumbnail after its render lands (thumb-rendered bump)", () => {
    render(ThumbnailGrid, { items: makeItems(20) });

    const img = document.querySelector(
      '[data-testid="grid-cell"] img',
    ) as HTMLImageElement;
    expect(img.getAttribute("src")).toBe(`thumb://${HASH(0)}`);

    // The backend emitted thumb-rendered for this hash: the store bumps,
    // the src changes, the WebView re-requests (cold-start fix).
    gridState.bumpThumbVersion(HASH(0));
    flushSync();

    expect(img.getAttribute("src")).toBe(`thumb://${HASH(0)}?v=1`);
  });

  it("reports the visible hashes, and re-reports when the scroll range moves", async () => {
    const onvisible = vi.fn();
    render(ThumbnailGrid, {
      items: makeItems(1000),
      onvisible,
    });

    // Reporting is debounced (100 ms) so fast scrolling does not spam IPC.
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

  // Viewer open (TASK-01-019): double-click (or Enter on the focused cell)
  // hands the photo to the shell, which mounts the full-view viewer over
  // the current collection.
  it("double-click opens the photo in the viewer (onopen)", () => {
    const onopen = vi.fn();
    render(ThumbnailGrid, { items: makeItems(10), onopen });

    const cell = document.querySelectorAll('[data-testid="grid-cell"]')[3];
    cell.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    flushSync();
    expect(onopen).toHaveBeenCalledWith({
      id: 3,
      filename: "photo-3.jpg",
      hash: HASH(3),
    });
  });

  it("Enter on a focused cell opens it too", () => {
    const onopen = vi.fn();
    render(ThumbnailGrid, { items: makeItems(10), onopen });

    const cell = document.querySelectorAll(
      '[data-testid="grid-cell"]',
    )[1] as HTMLElement;
    cell.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    flushSync();
    expect(onopen).toHaveBeenCalledOnce();
  });

  // Context menu (TASK-01-021): right-clicking a cell reports the cell's
  // position and the photo, so the shell can open its per-photo menu.
  it("right-click reports the photo and pointer position (oncontextmenu)", () => {
    const oncontextmenu = vi.fn();
    render(ThumbnailGrid, { items: makeItems(10), oncontextmenu });

    const cell = document.querySelectorAll(
      '[data-testid="grid-cell"]',
    )[2] as HTMLElement;
    const ev = new MouseEvent("contextmenu", {
      bubbles: true,
      clientX: 11,
      clientY: 22,
    });
    cell.dispatchEvent(ev);
    flushSync();
    expect(oncontextmenu).toHaveBeenCalledWith({
      x: 11,
      y: 22,
      item: { id: 2, filename: "photo-2.jpg", hash: HASH(2) },
    });
  });
});

describe("ThumbnailGrid hidden photos (TASK-03-009)", () => {
  it("marks revealed hidden photos so they render grayed", () => {
    const items: GridItem[] = [
      { id: 1, filename: "open.jpg", hash: HASH(1) },
      { id: 2, filename: "secret.jpg", hash: HASH(2), hidden: true },
    ];
    render(ThumbnailGrid, { items });
    const cells = screen.getAllByTestId("grid-cell");
    expect(cells[0].getAttribute("data-hidden")).toBeNull();
    expect(cells[1].getAttribute("data-hidden")).toBe("true");
  });
});

describe("ThumbnailGrid captions (TASK-03-015)", () => {
  const captioned = (): GridItem[] => [
    {
      id: 1,
      filename: "a.jpg",
      hash: HASH(1),
      caption: "Sunset",
      tags: ["beach", "Vacation|Italy"],
      width: 4000,
      height: 3000,
    },
    { id: 2, filename: "b.jpg", hash: HASH(2) },
  ];

  const captions = () => screen.queryAllByTestId("grid-caption").map((el) => el.textContent);

  it("renders no caption strip in none mode", () => {
    render(ThumbnailGrid, { items: captioned() });
    expect(captions()).toEqual([]);
  });

  it("switches through all five modes instantly, from the toolbar", async () => {
    render(ThumbnailGrid, { items: captioned() });
    const select = screen.getByTestId("caption-mode") as HTMLSelectElement;
    const expected: Record<string, (string | null)[]> = {
      filename: ["a.jpg", "b.jpg"],
      caption: ["Sunset", ""],
      tags: ["beach, Italy", ""],
      resolution: ["4000 × 3000", ""],
      none: [],
    };
    for (const [mode, texts] of Object.entries(expected)) {
      await fireEvent.change(select, { target: { value: mode } });
      flushSync();
      expect(gridState.captionMode).toBe(mode);
      expect(captions()).toEqual(texts);
    }
  });

  it("reserves the same strip height for every caption mode (no layout thrash)", () => {
    gridState.captionMode = "filename";
    render(ThumbnailGrid, { items: captioned() });
    const rowHeight = () => screen.getAllByTestId("grid-cell")[0].parentElement?.style.height;
    const filenameHeight = rowHeight();
    expect(filenameHeight).toBe("186px"); // 160 + 8 gap + 18 strip

    gridState.captionMode = "tags";
    flushSync();
    flushSync();
    expect(rowHeight()).toBe(filenameHeight);
  });

  it("keeps the original row height in none mode", () => {
    render(ThumbnailGrid, { items: captioned() });
    expect(screen.getAllByTestId("grid-cell")[0].parentElement?.style.height).toBe("168px");
  });

  it("exposes the full text as a tooltip so ellipsized values stay readable", () => {
    gridState.captionMode = "caption";
    render(ThumbnailGrid, { items: captioned() });
    expect(screen.getAllByTestId("grid-caption")[0].getAttribute("title")).toBe("Sunset");
  });
});
