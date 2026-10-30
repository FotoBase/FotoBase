// TASK-04-006 tests: pure geometry + op emission for the red-eye Basic
// Fixes controls. Regions are normalized circles (edit-recipe spec §2), the
// same values feed the recipe op and the overlay preview.

import { describe, expect, it } from "vitest";

import { clampRegion, opsFromState, regionContains, regionFromDrag } from "./red-eye";

describe("clampRegion", () => {
  it("keeps a valid region untouched", () => {
    expect(clampRegion({ x: 0.5, y: 0.5, r: 0.05 })).toEqual({ x: 0.5, y: 0.5, r: 0.05 });
  });

  it("pulls the center back inside the image and bounds the radius", () => {
    expect(clampRegion({ x: -0.1, y: 1.2, r: 0.05 })).toEqual({ x: 0, y: 1, r: 0.05 });
    expect(clampRegion({ x: 0.5, y: 0.5, r: 0.001 }).r).toBeGreaterThan(0);
    expect(clampRegion({ x: 0.5, y: 0.5, r: 0.9 }).r).toBeLessThanOrEqual(0.2);
  });
});

describe("regionFromDrag", () => {
  it("centers the circle on the drag midpoint with half-drag radius", () => {
    const r = regionFromDrag({ x: 0.4, y: 0.5 }, { x: 0.5, y: 0.5 });
    expect(r.x).toBeCloseTo(0.45);
    expect(r.y).toBeCloseTo(0.5);
    expect(r.r).toBeCloseTo(0.05);
  });

  it("never produces a zero radius", () => {
    const r = regionFromDrag({ x: 0.5, y: 0.5 }, { x: 0.5, y: 0.5 });
    expect(r.r).toBeGreaterThan(0);
  });
});

describe("regionContains", () => {
  it("is true inside the circle and false outside", () => {
    const r = { x: 0.5, y: 0.5, r: 0.1 };
    expect(regionContains(r, 0.55, 0.5)).toBe(true);
    expect(regionContains(r, 0.9, 0.9)).toBe(false);
  });
});

describe("opsFromState", () => {
  it("emits nothing for the untouched default state", () => {
    expect(opsFromState({ auto: false, regions: [] })).toEqual([]);
  });

  it("emits the auto op for one-click mode", () => {
    expect(opsFromState({ auto: true, regions: [] })).toEqual([
      { type: "red_eye", params: { auto: true } },
    ]);
  });

  it("emits the manual regions op otherwise", () => {
    expect(
      opsFromState({ auto: false, regions: [{ x: 0.3, y: 0.4, r: 0.05 }] }),
    ).toEqual([{ type: "red_eye", params: { regions: [{ x: 0.3, y: 0.4, r: 0.05 }] } }]);
  });
});
