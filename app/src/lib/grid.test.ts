// Pure grid math for the virtualized thumbnail grid (TASK-01-013, PLAN §4).
// Unit-tested here so the Svelte component only wires state.

import { describe, expect, it, vi } from "vitest";
import { columnCount, debounce, rowItems, thumbUri, visibleHashes, type GridItem } from "./grid";

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
    expect(visibleHashes(items, 2, [0, 1])).toEqual(["a".repeat(32), "b".repeat(32)]);
  });

  it("returns an empty list for an empty visible range", () => {
    expect(visibleHashes(items, 2, [])).toEqual([]);
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
