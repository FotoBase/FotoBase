// TASK-04-005 tests: pure geometry + op emission for the crop/straighten
// Basic Fixes controls. All geometry is normalized (edit-recipe spec §2) so
// the same values feed the recipe ops and the live preview.

import { describe, expect, it } from "vitest";

import {
  ASPECT_PRESETS,
  applyAspectPreset,
  autoFillScale,
  clampCrop,
  maxRectWithRatio,
  opsFromState,
  resizeCrop,
  type CropRect,
} from "./crop-straighten";

const rect = (x: number, y: number, w: number, h: number): CropRect => ({ x, y, w, h });

describe("clampCrop", () => {
  it("keeps a valid rect untouched", () => {
    expect(clampCrop(rect(0.2, 0.2, 0.5, 0.5))).toEqual(rect(0.2, 0.2, 0.5, 0.5));
  });

  it("pulls an out-of-bounds rect back inside the image", () => {
    expect(clampCrop(rect(-0.1, 0.9, 0.5, 0.5))).toEqual(rect(0, 0.5, 0.5, 0.5));
    expect(clampCrop(rect(0.8, 0.2, 0.5, 0.5))).toEqual(rect(0.5, 0.2, 0.5, 0.5));
  });

  it("enforces a minimum size so the crop can never collapse", () => {
    const out = clampCrop(rect(0.4, 0.4, 0.0001, 0.0001));
    expect(out.w).toBeGreaterThan(0);
    expect(out.h).toBeGreaterThan(0);
  });
});

describe("maxRectWithRatio", () => {
  it("fills the image when the ratio matches", () => {
    expect(maxRectWithRatio(1, 1)).toEqual(rect(0, 0, 1, 1));
  });

  it("centers the largest 1:1 rect in a landscape image", () => {
    expect(maxRectWithRatio(2, 1)).toEqual(rect(0.25, 0, 0.5, 1));
  });

  it("centers the largest 4:6 (portrait) rect in a landscape image", () => {
    // Height-bound: h = 1, w = (4/6)/2 ≈ 0.333 of the image width.
    const out = maxRectWithRatio(2, 4 / 6);
    expect(out.h).toBeCloseTo(1);
    expect(out.w).toBeCloseTo(4 / 6 / 2);
    expect(out.x).toBeCloseTo((1 - 4 / 6 / 2) / 2);
    expect(out.y).toBeCloseTo(0);
  });
});

describe("applyAspectPreset", () => {
  it("resets to the full image for the free preset", () => {
    expect(applyAspectPreset(rect(0.1, 0.1, 0.3, 0.3), null)).toEqual(rect(0, 0, 1, 1));
  });

  it("snaps to the largest matching rect for the image aspect", () => {
    const out = applyAspectPreset(rect(0.25, 0.25, 0.5, 0.5), 1, 2);
    expect(out).toEqual(rect(0.25, 0, 0.5, 1));
  });
});

describe("resizeCrop", () => {
  it("grows the rect when dragging the bottom-right handle out", () => {
    const out = resizeCrop(rect(0.2, 0.2, 0.3, 0.3), "br", 0.2, 0.2, null);
    expect(out).toEqual(rect(0.2, 0.2, 0.5, 0.5));
  });

  it("moves the top-left corner when dragging it, keeping the br corner", () => {
    const out = resizeCrop(rect(0.2, 0.2, 0.3, 0.3), "tl", -0.1, -0.1, null);
    expect(out).toEqual(rect(0.1, 0.1, 0.4, 0.4));
  });

  it("locks to the aspect ratio after a preset", () => {
    const start = maxRectWithRatio(1, 1); // 0,0,1,1
    const out = resizeCrop(start, "br", -0.2, 0, 1);
    expect(out.w).toBeCloseTo(out.h);
    expect(out.x).toBeCloseTo(0);
    expect(out.y).toBeCloseTo(0);
  });

  it("clamps the result inside the image", () => {
    const out = resizeCrop(rect(0.8, 0.8, 0.2, 0.2), "br", 1, 1, null);
    expect(out.x + out.w).toBeLessThanOrEqual(1);
    expect(out.y + out.h).toBeLessThanOrEqual(1);
  });
});

describe("autoFillScale", () => {
  it("is 1 for a zero angle", () => {
    expect(autoFillScale(0, 2, 1)).toBe(1);
  });

  it("is 1 when the rotated frame still fits", () => {
    expect(autoFillScale(0.001, 2, 1)).toBeCloseTo(1, 4);
  });

  it("shrinks content for a real angle so no corner is empty", () => {
    // 10° on a square: need = cos10 + sin10 ≈ 1.1585 → scale ≈ 0.8632.
    expect(autoFillScale(10, 1, 1)).toBeCloseTo(1 / (Math.cos(10 * Math.PI / 180) + Math.sin(10 * Math.PI / 180)), 6);
  });

  it("is 1 when auto-crop is disabled", () => {
    expect(autoFillScale(10, 1, 1, false)).toBe(1);
  });
});

describe("opsFromState", () => {
  it("emits nothing for the untouched state", () => {
    expect(opsFromState({ crop: rect(0, 0, 1, 1), angle: 0 })).toEqual([]);
  });

  it("emits a crop op with normalized params", () => {
    expect(opsFromState({ crop: rect(0.1, 0.2, 0.6, 0.6), angle: 0 })).toEqual([
      { type: "crop", params: { x: 0.1, y: 0.2, w: 0.6, h: 0.6 } },
    ]);
  });

  it("emits crop before straighten (spec §3: crop applies first)", () => {
    expect(opsFromState({ crop: rect(0.1, 0.1, 0.8, 0.8), angle: 3.5 })).toEqual([
      { type: "crop", params: { x: 0.1, y: 0.1, w: 0.8, h: 0.8 } },
      { type: "straighten", params: { angle: 3.5, auto_fill: true } },
    ]);
  });

  it("honors autoFill=false on the straighten op", () => {
    expect(opsFromState({ crop: rect(0, 0, 1, 1), angle: -2, autoFill: false })).toEqual([
      { type: "straighten", params: { angle: -2, auto_fill: false } },
    ]);
  });
});

describe("ASPECT_PRESETS", () => {
  it("offers free plus the documented ratios", () => {
    expect(ASPECT_PRESETS.map((p) => p.id)).toEqual([
      "free",
      "1:1",
      "4:3",
      "3:4",
      "6:4",
      "4:6",
      "16:9",
    ]);
  });
});
