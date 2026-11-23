// TASK-08-018 tests: the timeline's pure layout, hit-testing and keyboard
// stepping. A 3-year fixture must land groups at ascending, date-ordered
// positions; the visible window must stay a small slice of a 50k timeline.

import { describe, expect, it } from "vitest";

import {
  CELL,
  GAP_MAX,
  groupIndexAt,
  layoutTimeline,
  PAGE_STEP,
  scrollLeftFor,
  stepIndex,
  visibleRange,
  type TimelineGroup,
} from "./timeline";

const g = (date: string, id = 1): TimelineGroup => ({ date, count: 1, first_id: id, hash: null });

const THREE_YEARS = [g("2022-01-15"), g("2022-01-16"), g("2023-07-01"), g("2024-03-09")];

describe("layoutTimeline", () => {
  it("places groups left to right in date order, without overlap", () => {
    const { xs } = layoutTimeline(THREE_YEARS);
    for (let i = 1; i < xs.length; i++) expect(xs[i]).toBeGreaterThanOrEqual(xs[i - 1] + CELL);
  });

  it("spaces groups by the days between them, capped", () => {
    const { xs } = layoutTimeline(THREE_YEARS);
    const adjacent = xs[1] - xs[0];
    const farApart = xs[2] - xs[1];
    expect(farApart).toBeGreaterThan(adjacent);
    expect(farApart).toBe(CELL + GAP_MAX);
  });

  it("ticks every year and every month boundary at its first group", () => {
    const { ticks, xs } = layoutTimeline(THREE_YEARS);
    expect(ticks.filter((t) => t.kind === "year").map((t) => t.label)).toEqual([
      "2022",
      "2023",
      "2024",
    ]);
    expect(ticks[0].x).toBe(xs[0]);
    const july = layoutTimeline([g("2023-06-30"), g("2023-07-01")]).ticks;
    expect(july.map((t) => t.kind)).toEqual(["year", "month"]);
  });

  it("is empty for no groups", () => {
    expect(layoutTimeline([])).toMatchObject({ xs: [], totalWidth: 0, ticks: [] });
  });
});

describe("visibleRange / groupIndexAt", () => {
  const many = Array.from({ length: 50_000 }, (_, i) => {
    const d = new Date(Date.UTC(2000, 0, 1 + i));
    return g(d.toISOString().slice(0, 10), i);
  });
  const layout = layoutTimeline(many);

  it("returns only the groups on screen for a 50k timeline", () => {
    const [start, end] = visibleRange(layout, 1_000_000, 1200);
    expect(end - start).toBeLessThan(40);
    expect(layout.xs[start] + CELL).toBeGreaterThanOrEqual(1_000_000 - CELL);
    expect(layout.xs[end - 1]).toBeLessThanOrEqual(1_000_000 + 1200 + CELL);
  });

  it("covers the whole list when the viewport is wider than the content", () => {
    const small = layoutTimeline(THREE_YEARS);
    expect(visibleRange(small, 0, 100_000)).toEqual([0, 4]);
  });

  it("hit-tests a cell and misses the gap between cells", () => {
    const small = layoutTimeline(THREE_YEARS);
    expect(groupIndexAt(small, small.xs[2] + 5)).toBe(2);
    expect(groupIndexAt(small, small.xs[2] - 1)).toBe(-1);
    expect(groupIndexAt(small, small.xs[2] + CELL + 1)).toBe(-1);
  });
});

describe("stepIndex", () => {
  it("steps by one, clamped to the ends", () => {
    expect(stepIndex(1, "ArrowRight", 4)).toBe(2);
    expect(stepIndex(3, "ArrowRight", 4)).toBe(3);
    expect(stepIndex(0, "ArrowLeft", 4)).toBe(0);
    expect(stepIndex(2, "ArrowLeft", 4)).toBe(1);
  });

  it("starts at the first group when nothing is focused", () => {
    expect(stepIndex(-1, "ArrowRight", 4)).toBe(0);
    expect(stepIndex(-1, "ArrowLeft", 4)).toBe(0);
  });

  it("pages, and jumps to the ends", () => {
    expect(stepIndex(0, "PageDown", 100)).toBe(PAGE_STEP);
    expect(stepIndex(5, "PageUp", 100)).toBe(0);
    expect(stepIndex(7, "Home", 100)).toBe(0);
    expect(stepIndex(7, "End", 100)).toBe(99);
  });

  it("ignores other keys and an empty timeline", () => {
    expect(stepIndex(0, "a", 4)).toBeNull();
    expect(stepIndex(0, "ArrowRight", 0)).toBeNull();
  });
});

describe("scrollLeftFor", () => {
  it("centres the group, clamped to the scroll range", () => {
    const layout = layoutTimeline(THREE_YEARS);
    expect(scrollLeftFor(layout, 0, 400)).toBe(0);
    const last = scrollLeftFor(layout, 3, 400);
    expect(last).toBe(layout.totalWidth - 400);
  });
});
