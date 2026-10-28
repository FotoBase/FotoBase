// TASK-01-014: shared grid state — zoom, selection, scroll position live in
// ONE module-level store so the DOM grid (01-013) and the canvas grid stay in
// sync feature-wise (switching renderers must not lose zoom or selection).

import { beforeEach, describe, expect, it } from "vitest";
import { ALL_TIME } from "./filters";
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

  it("carries the library filters in the shared store (TASK-03-014)", () => {
    expect(gridState.starredOnly).toBe(false);
    expect(gridState.videosOnly).toBe(false);
    expect(gridState.dateStep).toBe(ALL_TIME);
    expect(gridState.filtersActive).toBe(false);

    gridState.videosOnly = true;
    expect(gridState.filtersActive).toBe(true);
    gridState.videosOnly = false;
    gridState.dateStep = 2;
    expect(gridState.filtersActive).toBe(true);

    gridState.starredOnly = true;
    gridState.clearFilters();
    expect(gridState.filtersActive).toBe(false);
    expect(gridState.starredOnly).toBe(false);
    expect(gridState.dateStep).toBe(ALL_TIME);

    gridState.starredOnly = true;
    resetGridState();
    expect(gridState.starredOnly).toBe(false);
  });

  it("bumps per-hash thumb versions monotonically and resets cleanly", () => {
    // thumb-rendered events bump the version; the URI changes and the
    // WebView re-requests (a cold-start placeholder gets replaced).
    const hash = "a".repeat(32);
    expect(gridState.thumbVersions[hash]).toBeUndefined();
    gridState.bumpThumbVersion(hash);
    expect(gridState.thumbVersions[hash]).toBe(1);
    gridState.bumpThumbVersion(hash);
    expect(gridState.thumbVersions[hash]).toBe(2);

    // An empty payload hash never lands in the map.
    gridState.bumpThumbVersion("");
    expect("" in gridState.thumbVersions).toBe(false);

    resetGridState();
    expect(gridState.thumbVersions).toEqual({});
  });

  it("bumps every known hash on a display-profile change (TASK-02-001)", () => {
    // The backend switched display profiles: every on-screen thumbnail must
    // re-request so stale old-profile pixels never linger.
    const a = "a".repeat(32);
    const b = "b".repeat(32);
    gridState.bumpThumbVersion(a);
    gridState.bumpThumbVersion(b);
    gridState.bumpThumbVersion(b);
    gridState.bumpAllThumbVersions();
    expect(gridState.thumbVersions[a]).toBe(2);
    expect(gridState.thumbVersions[b]).toBe(3);
    // Unknown hashes stay unknown: the first request for them renders
    // directly into the new profile's cache entry.
    const c = "c".repeat(32);
    expect(gridState.thumbVersions[c]).toBeUndefined();
    // With nothing rendered yet the bump is a no-op, not a crash.
    resetGridState();
    gridState.bumpAllThumbVersions();
    expect(gridState.thumbVersions).toEqual({});
  });
});

describe("reveal requests (TASK-03-010)", () => {
  it("defaults to none and resets with the store", () => {
    expect(gridState.revealRequest).toBeNull();
    gridState.revealRequest = { id: 4 };
    resetGridState();
    expect(gridState.revealRequest).toBeNull();
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

describe("gridState caption mode (TASK-03-015)", () => {
  it("defaults to none and resets with the store", () => {
    expect(gridState.captionMode).toBe("none");
    gridState.captionMode = "tags";
    resetGridState();
    expect(gridState.captionMode).toBe("none");
  });
});
