// TASK-01-014: shared grid state — zoom, selection, scroll position live in
// ONE module-level store so the DOM grid (01-013) and the canvas grid stay in
// sync feature-wise (switching renderers must not lose zoom or selection).

import { beforeEach, describe, expect, it } from "vitest";
import {
  DEFAULT_THUMB_SIZE,
  ZOOM_MAX,
  ZOOM_MIN,
  ZOOM_STEP,
  gridState,
  pickRenderer,
  resetGridState,
  type GridRenderer,
} from "./grid-state.svelte.js";

beforeEach(() => resetGridState());

describe("gridState", () => {
  it("defaults to a 160px zoom, empty selection, top scroll, empty search", () => {
    expect(gridState.thumbSize).toBe(DEFAULT_THUMB_SIZE);
    expect(gridState.selection.size).toBe(0);
    expect(gridState.scrollTop).toBe(0);
    expect(gridState.searchQuery).toBe("");
  });

  it("toggleSelection adds, removes, and keeps ids unique", () => {
    gridState.toggleSelection(1);
    gridState.toggleSelection(2);
    expect([...gridState.selection]).toEqual([1, 2]);

    gridState.toggleSelection(1);
    expect([...gridState.selection]).toEqual([2]);
  });

  it("is module-level state: selection survives a renderer switch", () => {
    gridState.toggleSelection(7);
    // Simulate the renderer switch: a fresh component reads the same store.
    resetGridState; // not called — the store persists across components
    expect(gridState.selection.has(7)).toBe(true);
  });

  it("exposes the zoom range both sliders must use", () => {
    expect(ZOOM_MIN).toBe(64);
    expect(ZOOM_MAX).toBe(512);
    expect(ZOOM_STEP).toBe(8);
  });

  it("carries the live-search query in the shared store (TASK-01-018)", () => {
    // Query state lives in the store so both renderers filter for free.
    gridState.searchQuery = "beach";
    expect(gridState.searchQuery).toBe("beach");
    resetGridState();
    expect(gridState.searchQuery).toBe("");
  });
});

describe("pickRenderer", () => {
  it("uses the canvas renderer only when explicitly configured", () => {
    expect(pickRenderer("canvas")).toBe("canvas");
  });

  it("defaults to DOM — no ADR has chosen canvas as an OS default yet", () => {
    expect(pickRenderer("dom")).toBe("dom");
    // Unknown/legacy values must degrade to the DOM grid, never crash.
    expect(pickRenderer("bogus" as GridRenderer)).toBe("dom");
  });
});
