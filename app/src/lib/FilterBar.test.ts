// FilterBar tests (TASK-03-014, PLAN §2 C "Filter buttons" + "Date slider"):
// the starred / videos toggles and the date slider write the shared grid
// store; the shell re-queries from there, so grid and viewer (which both read
// the shell's result) filter identically. The slider is debounced; the
// toggles apply immediately; each filter clears in one click.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import FilterBar from "./FilterBar.svelte";
import { ALL_TIME, DATE_STEPS } from "./filters";
import { gridState, resetGridState } from "./grid-state.svelte.js";

beforeEach(() => {
  vi.useFakeTimers();
  resetGridState();
});

afterEach(() => {
  vi.useRealTimers();
  cleanup();
});

const DEBOUNCE_MS = 80;

describe("FilterBar", () => {
  it("renders both toggles unpressed and the slider at 'all time'", () => {
    render(FilterBar);
    expect(screen.getByTestId("filter-starred").getAttribute("aria-pressed")).toBe("false");
    expect(screen.getByTestId("filter-videos").getAttribute("aria-pressed")).toBe("false");
    const slider = screen.getByTestId("filter-date") as HTMLInputElement;
    expect(slider.value).toBe(String(ALL_TIME));
    expect(slider.max).toBe(String(DATE_STEPS.length - 1));
    expect(screen.queryByTestId("filter-date-clear")).toBeNull();
  });

  it("starred toggle flips the store immediately and back on a second click", () => {
    render(FilterBar);
    const btn = screen.getByTestId("filter-starred");

    fireEvent.click(btn);
    flushSync();
    expect(gridState.starredOnly).toBe(true);
    expect(btn.getAttribute("aria-pressed")).toBe("true");

    fireEvent.click(btn);
    flushSync();
    expect(gridState.starredOnly).toBe(false);
    expect(btn.getAttribute("aria-pressed")).toBe("false");
  });

  it("sort select switches between path and capture time (TASK-03-019)", () => {
    render(FilterBar);
    const select = screen.getByTestId("filter-sort") as HTMLSelectElement;
    expect(select.value).toBe("path");

    fireEvent.change(select, { target: { value: "capture" } });
    flushSync();
    expect(gridState.sort).toBe("capture");
    // The sort is not a filter: it never lights the "filters active" state.
    expect(gridState.filtersActive).toBe(false);
  });

  it("videos toggle is independent of the starred toggle", () => {
    render(FilterBar);
    fireEvent.click(screen.getByTestId("filter-videos"));
    flushSync();
    expect(gridState.videosOnly).toBe(true);
    expect(gridState.starredOnly).toBe(false);
  });

  it("debounces the slider into the store", () => {
    render(FilterBar);
    const slider = screen.getByTestId("filter-date");

    fireEvent.input(slider, { target: { value: "3" } });
    expect(gridState.dateStep).toBe(ALL_TIME);
    vi.advanceTimersByTime(DEBOUNCE_MS - 1);
    expect(gridState.dateStep).toBe(ALL_TIME);
    vi.advanceTimersByTime(1);
    expect(gridState.dateStep).toBe(3);
  });

  it("collapses a slider drag into its last position", () => {
    render(FilterBar);
    const slider = screen.getByTestId("filter-date");
    for (const v of ["1", "2", "4"]) fireEvent.input(slider, { target: { value: v } });
    vi.advanceTimersByTime(DEBOUNCE_MS);
    expect(gridState.dateStep).toBe(4);
  });

  it("shows a one-click clear for the date filter that resets slider and store", () => {
    render(FilterBar);
    const slider = screen.getByTestId("filter-date") as HTMLInputElement;
    fireEvent.input(slider, { target: { value: "2" } });
    vi.advanceTimersByTime(DEBOUNCE_MS);
    flushSync();

    fireEvent.click(screen.getByTestId("filter-date-clear"));
    flushSync();
    expect(gridState.dateStep).toBe(ALL_TIME);
    expect(slider.value).toBe(String(ALL_TIME));
    expect(screen.queryByTestId("filter-date-clear")).toBeNull();
  });

  it("a pending slider debounce cannot resurrect a cleared date filter", () => {
    render(FilterBar);
    const slider = screen.getByTestId("filter-date");
    fireEvent.input(slider, { target: { value: "2" } });
    vi.advanceTimersByTime(DEBOUNCE_MS);
    flushSync();
    fireEvent.input(slider, { target: { value: "5" } });
    fireEvent.click(screen.getByTestId("filter-date-clear"));
    vi.advanceTimersByTime(DEBOUNCE_MS);
    expect(gridState.dateStep).toBe(ALL_TIME);
  });

  it("reflects store state set elsewhere (session persistence across remounts)", () => {
    gridState.starredOnly = true;
    gridState.dateStep = 2;
    render(FilterBar);
    expect(screen.getByTestId("filter-starred").getAttribute("aria-pressed")).toBe("true");
    expect((screen.getByTestId("filter-date") as HTMLInputElement).value).toBe("2");
  });
});
