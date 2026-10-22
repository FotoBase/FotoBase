// TASK-01-013 component tests: virtualized DOM grid, bounded DOM under fast
// scroll, live zoom slider, visible-range reporting.
//
// jsdom has no layout, so the scroll element's dimensions come from the
// test-setup prototypes (800×600 unless a test overrides `__clientWidth` /
// `__clientHeight`). Rows are fixed-size (no dynamic measurement), which is
// also what keeps the grid cheap on 50k items.

import { flushSync } from "svelte";
import { cleanup, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import ThumbnailGrid from "./ThumbnailGrid.svelte";
import type { GridItem } from "./grid";

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

afterEach(() => cleanup());

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

    expect(document.querySelector('[data-testid="grid-placeholder"]')).not.toBeNull();
  });

  it("zoom slider resizes thumbs live without re-fetching (srcs unchanged)", () => {
    render(ThumbnailGrid, { items: makeItems(500) });

    const srcsBefore = [...document.querySelectorAll('[data-testid="grid-cell"] img')].map(
      (img) => img.getAttribute("src"),
    );

    const slider = screen.getByTestId("zoom-slider") as HTMLInputElement;
    slider.value = "320";
    slider.dispatchEvent(new Event("input"));
    flushSync();
    flushSync();

    const srcsAfter = [...document.querySelectorAll('[data-testid="grid-cell"] img')].map(
      (img) => img.getAttribute("src"),
    );
    // Zooming must not re-fetch: every rendered src was already rendered at
    // the old zoom level (fewer cells fit on screen at 320px, so a strict
    // subset — no new content fetched, none dropped that was still visible).
    expect(srcsAfter.length).toBeGreaterThan(0);
    expect(srcsBefore).toEqual(expect.arrayContaining(srcsAfter));

    // The cells now size from the new zoom level.
    const cell = document.querySelector('[data-testid="grid-cell"]') as HTMLElement;
    expect(cell.style.getPropertyValue("--thumb-size")).toBe("320px");
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
    expect(onopen).toHaveBeenCalledWith({ id: 3, filename: "photo-3.jpg", hash: HASH(3) });
  });

  it("Enter on a focused cell opens it too", () => {
    const onopen = vi.fn();
    render(ThumbnailGrid, { items: makeItems(10), onopen });

    const cell = document.querySelectorAll('[data-testid="grid-cell"]')[1] as HTMLElement;
    cell.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    flushSync();
    expect(onopen).toHaveBeenCalledOnce();
  });
});
