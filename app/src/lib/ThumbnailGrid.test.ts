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
  vi.unstubAllGlobals();
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

describe("ThumbnailGrid single-click-to-edit (TASK-03-017)", () => {
  const click = (init: MouseEventInit = {}): void => {
    const cell = screen.getAllByTestId("grid-cell")[0];
    cell.dispatchEvent(new MouseEvent("click", { bubbles: true, ...init }));
    flushSync();
  };

  it("option off: a click selects and does not open", () => {
    const onopen = vi.fn();
    render(ThumbnailGrid, { items: makeItems(10), onopen });
    click();
    expect(onopen).not.toHaveBeenCalled();
    expect([...gridState.selection]).toEqual([0]);
  });

  it("option on: a plain click opens the photo", () => {
    const onopen = vi.fn();
    render(ThumbnailGrid, { items: makeItems(10), onopen, singleClickEdit: true });
    click();
    expect(onopen).toHaveBeenCalledTimes(1);
    expect(onopen.mock.calls[0][0].id).toBe(0);
  });

  it("option on: modifier-click toggles selection instead of opening", () => {
    const onopen = vi.fn();
    render(ThumbnailGrid, { items: makeItems(10), onopen, singleClickEdit: true });
    click({ ctrlKey: true });
    expect([...gridState.selection]).toEqual([0]);
    click({ metaKey: true });
    expect(gridState.selection.size).toBe(0);
    click({ shiftKey: true });
    expect([...gridState.selection]).toEqual([0]);
    expect(onopen).not.toHaveBeenCalled();
  });
});

describe("ThumbnailGrid touch browsing (TASK-09-003)", () => {
  it("opens a photo on one tap when the primary pointer is coarse", () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn((query: string) => ({
        matches: query === "(pointer: coarse)",
        media: query,
        onchange: null,
        addListener: vi.fn(),
        removeListener: vi.fn(),
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    );
    const onopen = vi.fn();
    const oncontextmenu = vi.fn();
    render(ThumbnailGrid, { items: makeItems(10), onopen, oncontextmenu });

    const cell = screen.getAllByTestId("grid-cell")[1];
    fireEvent.click(cell);
    fireEvent.contextMenu(cell);
    flushSync();

    expect(onopen).toHaveBeenCalledWith(makeItems(10)[1]);
    expect(oncontextmenu).not.toHaveBeenCalled();
    expect(gridState.selection.size).toBe(0);
  });

  it("fits phone portrait and landscape widths using the shared column math", () => {
    const { container } = render(ThumbnailGrid, { items: makeItems(100) });
    const scroll = screen.getByTestId("grid-scroll") as HTMLElement & {
      __clientWidth: number;
    };
    const row = () => container.querySelector<HTMLElement>(".grid-row");

    scroll.__clientWidth = 390;
    window.dispatchEvent(new Event("resize"));
    flushSync();
    flushSync();
    expect(row()?.style.gridTemplateColumns).toBe("repeat(2, var(--thumb-size))");

    scroll.__clientWidth = 844;
    window.dispatchEvent(new Event("resize"));
    flushSync();
    flushSync();
    expect(row()?.style.gridTemplateColumns).toBe("repeat(5, var(--thumb-size))");
  });
});

describe("ThumbnailGrid touch star (TASK-09-004)", () => {
  const stubCoarse = (coarse: boolean): void => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn((query: string) => ({
        matches: coarse && query === "(pointer: coarse)",
        media: query,
        onchange: null,
        addListener: vi.fn(),
        removeListener: vi.fn(),
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    );
  };

  it("a touch device shows a star per cell; tapping it stars without opening", () => {
    stubCoarse(true);
    const onopen = vi.fn();
    const onstar = vi.fn();
    const items = makeItems(10).map((it, i) => (i === 1 ? { ...it, rating: 5 } : it));
    render(ThumbnailGrid, { items, onopen, onstar });

    const stars = screen.getAllByTestId("grid-star");
    expect(stars[0].getAttribute("aria-pressed")).toBe("false");
    expect(stars[1].getAttribute("aria-pressed")).toBe("true");

    fireEvent.click(stars[0]);
    flushSync();
    expect(onstar).toHaveBeenCalledWith(items[0]);
    expect(onopen).not.toHaveBeenCalled();
    expect(gridState.selection.size).toBe(0);
  });

  it("a mouse device has no grid star", () => {
    stubCoarse(false);
    render(ThumbnailGrid, { items: makeItems(10) });
    expect(screen.queryByTestId("grid-star")).toBeNull();
  });
});

describe("ThumbnailGrid stack badge (TASK-03-021)", () => {
  const stacked = (): GridItem[] => [
    { id: 1, filename: "a.jpg", hash: HASH(1), stack_id: "s1", stack_count: 3 },
    { id: 2, filename: "b.jpg", hash: HASH(2) },
  ];

  it("draws one badge with the stack size on a stacked photo only", () => {
    render(ThumbnailGrid, { items: stacked() });
    const badges = screen.getAllByTestId("stack-badge");
    expect(badges).toHaveLength(1);
    expect(badges[0].textContent).toBe("3");
  });

  it("clicking the badge opens the picker for that stack without selecting or opening", () => {
    const onopenstack = vi.fn();
    const onopen = vi.fn();
    render(ThumbnailGrid, { items: stacked(), onopenstack, onopen });

    const badge = screen.getByTestId("stack-badge");
    badge.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 40, clientY: 50 }));
    flushSync();

    expect(onopenstack).toHaveBeenCalledWith(
      expect.objectContaining({ stackId: "s1", x: expect.any(Number), y: expect.any(Number) }),
    );
    expect(onopen).not.toHaveBeenCalled();
    expect(gridState.selection.size).toBe(0);
  });
});

describe("ThumbnailGrid row layout", () => {
  it("defines --thumb-size on the row, where grid-template-columns reads it", () => {
    // `repeat(N, var(--thumb-size))` is invalid (one column, cells stacked)
    // when the property is only set on the cells below the row.
    const { container } = render(ThumbnailGrid, { items: makeItems(10) });
    const row = container.querySelector<HTMLElement>(".grid-row");
    expect(row).not.toBeNull();
    expect(row!.style.getPropertyValue("--thumb-size")).toBe(`${gridState.thumbSize}px`);
  });
});

describe("ThumbnailGrid live badge (TASK-08-017)", () => {
  it("badges Live and Motion photos only", () => {
    const items: GridItem[] = [
      { id: 1, filename: "a.heic", hash: HASH(1), live_kind: "live" },
      { id: 2, filename: "b.jpg", hash: HASH(2), live_kind: "motion" },
      { id: 3, filename: "c.jpg", hash: HASH(3), live_kind: null },
      { id: 4, filename: "d.jpg", hash: HASH(4) },
    ];
    render(ThumbnailGrid, { items });
    const badges = screen.getAllByTestId("live-badge");
    expect(badges.map((b) => b.textContent?.trim())).toEqual(["◉ Live", "◉ Motion"]);
  });
});
