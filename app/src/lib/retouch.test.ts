// TASK-08-002 tests: heal brush state → `retouch` recipe op. Strokes are
// normalized to the image (points = fractions of width/height, radius =
// fraction of the shorter side) so the Rust engine re-runs them identically
// at preview and export resolution.

import { describe, expect, it } from "vitest";

import {
  DEFAULT_RADIUS,
  MAX_RADIUS,
  MIN_RADIUS,
  clampRadius,
  opsFromRetouch,
  retouchFromOps,
  removeStroke,
  strokeFromPoints,
  undoLastStroke,
  type RetouchState,
} from "./retouch";

describe("retouch", () => {
  it("an empty state emits no op", () => {
    expect(opsFromRetouch({ strokes: [] })).toEqual([]);
  });

  it("emits one retouch op carrying every stroke in order", () => {
    const state: RetouchState = {
      strokes: [
        { points: [[0.1, 0.2]], radius: 0.02 },
        { points: [[0.5, 0.5], [0.6, 0.5]], radius: 0.05 },
      ],
    };
    expect(opsFromRetouch(state)).toEqual([
      {
        type: "retouch",
        params: {
          strokes: [
            { points: [[0.1, 0.2]], radius: 0.02 },
            { points: [[0.5, 0.5], [0.6, 0.5]], radius: 0.05 },
          ],
        },
      },
    ]);
  });

  it("clamps the radius to the brush range", () => {
    expect(clampRadius(0)).toBe(MIN_RADIUS);
    expect(clampRadius(5)).toBe(MAX_RADIUS);
    expect(clampRadius(DEFAULT_RADIUS)).toBe(DEFAULT_RADIUS);
  });

  it("builds a stroke from a dragged path: clamped to the image, rounded, de-duplicated", () => {
    const stroke = strokeFromPoints(
      [
        { x: -0.2, y: 0.5 },
        { x: 0.123456, y: 0.5 },
        { x: 0.123457, y: 0.5 }, // same spot after rounding
        { x: 1.4, y: 0.5 },
      ],
      0.04,
    );
    expect(stroke).toEqual({
      points: [
        [0, 0.5],
        [0.1235, 0.5],
        [1, 0.5],
      ],
      radius: 0.04,
    });
  });

  it("a single click is a one-point stroke (a dab)", () => {
    expect(strokeFromPoints([{ x: 0.3, y: 0.7 }], 0.03)).toEqual({
      points: [[0.3, 0.7]],
      radius: 0.03,
    });
  });

  it("no points is no stroke", () => {
    expect(strokeFromPoints([], 0.03)).toBeNull();
  });

  it("removeStroke deletes only the chosen stroke", () => {
    const a = { points: [[0.1, 0.1]] as [number, number][], radius: 0.02 };
    const b = { points: [[0.5, 0.5]] as [number, number][], radius: 0.02 };
    const c = { points: [[0.9, 0.9]] as [number, number][], radius: 0.02 };
    const next = removeStroke({ strokes: [a, b, c] }, 1);
    expect(next.strokes).toEqual([a, c]);
    // out-of-range index: unchanged
    expect(removeStroke({ strokes: [a] }, 4).strokes).toEqual([a]);
  });

  it("undoLastStroke pops the newest stroke", () => {
    const a = { points: [[0.1, 0.1]] as [number, number][], radius: 0.02 };
    const b = { points: [[0.5, 0.5]] as [number, number][], radius: 0.02 };
    expect(undoLastStroke({ strokes: [a, b] }).strokes).toEqual([a]);
    expect(undoLastStroke({ strokes: [] }).strokes).toEqual([]);
  });

  it("restores strokes from stored op params (round trip)", () => {
    const state: RetouchState = {
      strokes: [{ points: [[0.25, 0.75], [0.3, 0.8]], radius: 0.06 }],
    };
    const op = opsFromRetouch(state)[0];
    expect(retouchFromOps([op.params])).toEqual(state);
  });

  it("merges several stored retouch ops and drops malformed strokes", () => {
    const restored = retouchFromOps([
      { strokes: [{ points: [[0.1, 0.1]], radius: 0.02 }] },
      {
        strokes: [
          { points: [], radius: 0.02 },
          { points: [[2, 0.5]], radius: 0.02 },
          { points: [[0.4, 0.4]] },
          "junk",
          { points: [[0.6, 0.6]], radius: 0.03 },
        ],
      },
      {},
    ]);
    expect(restored.strokes).toEqual([
      { points: [[0.1, 0.1]], radius: 0.02 },
      { points: [[0.6, 0.6]], radius: 0.03 },
    ]);
  });
});
