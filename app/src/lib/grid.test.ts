// Pure grid math for the virtualized thumbnail grid (TASK-01-013, PLAN §4).
// Unit-tested here so the Svelte component only wires state.

import { describe, expect, it, vi } from "vitest";
import {
  cellAt,
  columnCount,
  debounce,
  nextCell,
  rowItems,
  thumbSrcset,
  thumbUri,
  visibleHashes,
  type GridItem,
} from "./grid";

describe("columnCount", () => {
  it("fits as many full thumb+gap columns as the width allows", () => {
    // (800 + 8) / (160 + 8) = 4.8 -> 4 full columns.
    expect(columnCount(800, 160, 8)).toBe(4);
  });

  it("is at least one, even for a zero-width or oversized thumb", () => {
    expect(columnCount(0, 160, 8)).toBe(1);
    expect(columnCount(100, 500, 8)).toBe(1);
  });

  it("grows when thumbs shrink (zoom-out)", () => {
    expect(columnCount(800, 64, 8)).toBe(11);
  });
});

describe("thumbUri", () => {
  it("maps a valid cache key to a thumb:// URI (no IPC bytes, PLAN §3 rule 3)", () => {
    const hash = "a".repeat(32);
    expect(thumbUri(hash)).toBe(`thumb://${hash}`);
  });

  it("rejects malformed keys and nulls", () => {
    expect(thumbUri(null)).toBeNull();
    expect(thumbUri("short")).toBeNull();
    expect(thumbUri("../etc/passwd")).toBeNull();
    expect(thumbUri("A".repeat(32))).toBeNull(); // uppercase is not a cache key
  });

  it("appends a cache-bust ?v=N after a thumb-rendered bump", () => {
    const hash = "a".repeat(32);
    expect(thumbUri(hash, 1)).toBe(`thumb://${hash}?v=1`);
    expect(thumbUri(hash, 3)).toBe(`thumb://${hash}?v=3`);
    // Version 0 = never rendered since mount: plain URI, no query.
    expect(thumbUri(hash, 0)).toBe(`thumb://${hash}`);
  });
});

describe("thumbSrcset (HiDPI, TASK-01-024)", () => {
  const hash = "a".repeat(32);

  it("is null on 1x displays — the single src is already sharp", () => {
    expect(thumbSrcset(hash, 1)).toBeNull();
  });

  it("offers a 2x variant on HiDPI displays", () => {
    expect(thumbSrcset(hash, 2)).toBe(`thumb://${hash}?s=2 2x`);
    expect(thumbSrcset(hash, 1.5)).toBe(`thumb://${hash}?s=2 2x`);
  });

  it("rejects malformed hashes and nulls", () => {
    expect(thumbSrcset(null, 2)).toBeNull();
    expect(thumbSrcset("short", 2)).toBeNull();
    expect(thumbSrcset("A".repeat(32), 2)).toBeNull();
  });

  it("carries the cache-bust version alongside the scale param", () => {
    expect(thumbSrcset(hash, 2, 1)).toBe(`thumb://${hash}?v=1&s=2 2x`);
    expect(thumbSrcset(hash, 2, 0)).toBe(`thumb://${hash}?s=2 2x`);
  });
});

describe("nextCell (keyboard grid navigation, TASK-01-024)", () => {
  it("moves within a row", () => {
    expect(nextCell(0, 3, 7, "ArrowRight")).toBe(1);
    expect(nextCell(1, 3, 7, "ArrowLeft")).toBe(0);
  });

  it("never leaves the grid", () => {
    expect(nextCell(0, 3, 7, "ArrowLeft")).toBeNull();
    expect(nextCell(6, 3, 7, "ArrowRight")).toBeNull();
  });

  it("moves between rows and stops at the edges", () => {
    expect(nextCell(1, 3, 7, "ArrowDown")).toBe(4);
    expect(nextCell(4, 3, 7, "ArrowUp")).toBe(1);
    expect(nextCell(0, 3, 7, "ArrowUp")).toBeNull();
    // 5 + 3 = 8 is past the last item (7 items, indices 0..6).
    expect(nextCell(5, 3, 7, "ArrowDown")).toBeNull();
  });
});

describe("rowItems", () => {
  const items = [1, 2, 3, 4, 5, 6, 7];

  it("slices a full row", () => {
    expect(rowItems(items, 3, 0)).toEqual([1, 2, 3]);
    expect(rowItems(items, 3, 1)).toEqual([4, 5, 6]);
  });

  it("slices a partial last row", () => {
    expect(rowItems(items, 3, 2)).toEqual([7]);
  });

  it("is empty past the end", () => {
    expect(rowItems(items, 3, 3)).toEqual([]);
  });
});

describe("visibleHashes", () => {
  const items: GridItem[] = [
    { id: 1, filename: "a.jpg", hash: "a".repeat(32) },
    { id: 2, filename: "b.jpg", hash: "b".repeat(32) },
    { id: 3, filename: "c.jpg", hash: "a".repeat(32) }, // duplicate hash (same content)
    { id: 4, filename: "d.jpg", hash: null }, // not yet hashed
  ];

  it("collects the unique hashes of the visible rows, skipping unhashed files", () => {
    expect(visibleHashes(items, 2, [0, 1])).toEqual([
      "a".repeat(32),
      "b".repeat(32),
    ]);
  });

  it("returns an empty list for an empty visible range", () => {
    expect(visibleHashes(items, 2, [])).toEqual([]);
  });
});

describe("cellAt (canvas hit-testing, TASK-01-014)", () => {
  // 160px thumbs + 8px gap: cells occupy [0,160), [168,328), ...
  const size = 160;
  const cols = 4;
  const count = 9;

  it("maps a click inside the first cell to index 0", () => {
    expect(cellAt(10, 10, 0, size, cols, count)).toBe(0);
    expect(cellAt(159, 159, 0, size, cols, count)).toBe(0);
  });

  it("maps clicks further right and down via the scroll offset", () => {
    expect(cellAt(168, 10, 0, size, cols, count)).toBe(1);
    // Row 1 starts at y=168; with the viewport scrolled 168px down, a click
    // at viewport y=10 hits row 1.
    expect(cellAt(10, 10, 168, size, cols, count)).toBe(4);
  });

  it("returns null for clicks past the last column or the last item", () => {
    // 9 items over 4 columns = rows 0..2; row 2 holds only index 8 (col 0).
    expect(cellAt(168, 336, 0, size, cols, count)).toBeNull(); // row 2, col 1: past the last item
    expect(cellAt(10, 600, 0, size, cols, count)).toBeNull(); // row 3: empty
    expect(cellAt(700, 10, 0, size, cols, count)).toBeNull(); // col 4 does not exist
  });

  it("never returns a negative index", () => {
    expect(cellAt(10, 10, -168, size, cols, count)).toBeNull();
  });
});

describe("debounce", () => {
  it("collapses a burst into the last call", () => {
    vi.useFakeTimers();
    const fn = vi.fn();
    const debounced = debounce(fn, 100);
    debounced("a");
    debounced("b");
    debounced("c");
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(100);
    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenCalledWith("c");
    vi.useRealTimers();
  });
});
