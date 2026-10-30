// TASK-04-008 tests: pure state + op emission for the Tuning tab controls.
// Four sliders (Fill Light, Highlights, Shadows, Color Temperature) map to
// recipe ops; the neutral picker's computed balance lands on the
// temperature/tint params.

import { describe, expect, it } from "vitest";

import {
  applyBalance,
  clampedSlider,
  DEFAULT_TUNING,
  opsFromState,
  sampleNeutral,
  type TuningState,
} from "./tuning";

describe("opsFromState", () => {
  it("emits nothing for the untouched default state", () => {
    expect(opsFromState(DEFAULT_TUNING)).toEqual([]);
  });

  it("maps Fill Light to fill_light", () => {
    expect(opsFromState({ ...DEFAULT_TUNING, fillLight: 0.4 })).toEqual([
      { type: "fill_light", params: { amount: 0.4 } },
    ]);
  });

  it("maps Highlights to highlight_recovery", () => {
    expect(opsFromState({ ...DEFAULT_TUNING, highlights: 0.6 })).toEqual([
      { type: "highlight_recovery", params: { amount: 0.6 } },
    ]);
  });

  it("maps Shadows to shadow_recovery", () => {
    expect(opsFromState({ ...DEFAULT_TUNING, shadows: 0.3 })).toEqual([
      { type: "shadow_recovery", params: { amount: 0.3 } },
    ]);
  });

  it("maps Color Temperature to temperature", () => {
    expect(opsFromState({ ...DEFAULT_TUNING, temperature: -0.5 })).toEqual([
      { type: "temperature", params: { kelvin_shift: -0.5 } },
    ]);
  });

  it("emits tint for the picker's green/magenta balance", () => {
    expect(opsFromState({ ...DEFAULT_TUNING, tint: 0.2 })).toEqual([
      { type: "tint", params: { green_magenta: 0.2 } },
    ]);
  });

  it("emits ops in spec order when all sliders move", () => {
    const state: TuningState = {
      fillLight: 0.5,
      highlights: 0.2,
      shadows: 0.3,
      temperature: -0.1,
      tint: 0.05,
    };
    expect(opsFromState(state).map((o) => o.type)).toEqual([
      "fill_light",
      "highlight_recovery",
      "shadow_recovery",
      "temperature",
      "tint",
    ]);
  });
});

describe("clampedSlider", () => {
  it("keeps Fill Light in [0, 1]", () => {
    expect(clampedSlider("fillLight", -0.2)).toBe(0);
    expect(clampedSlider("fillLight", 1.4)).toBe(1);
    expect(clampedSlider("fillLight", 0.5)).toBe(0.5);
  });

  it("keeps Temperature/Tint in [-1, 1]", () => {
    expect(clampedSlider("temperature", -2)).toBe(-1);
    expect(clampedSlider("temperature", 1.5)).toBe(1);
    expect(clampedSlider("tint", 0.25)).toBe(0.25);
  });
});

describe("applyBalance", () => {
  it("a neutral balance leaves the sliders at zero", () => {
    const state = applyBalance(DEFAULT_TUNING, {
      kelvinShift: 0,
      greenMagenta: 0,
    });
    expect(state.temperature).toBe(0);
    expect(state.tint).toBe(0);
  });

  it("a computed balance lands on the temperature/tint sliders", () => {
    const state = applyBalance(DEFAULT_TUNING, {
      kelvinShift: -0.6,
      greenMagenta: -0.04,
    });
    expect(state.temperature).toBeCloseTo(-0.6);
    expect(state.tint).toBeCloseTo(-0.04);
  });

  it("replaces a previous picker balance", () => {
    const first = applyBalance(DEFAULT_TUNING, {
      kelvinShift: 0.4,
      greenMagenta: 0.1,
    });
    const second = applyBalance(first, { kelvinShift: -0.2, greenMagenta: 0 });
    expect(second.temperature).toBeCloseTo(-0.2);
    expect(second.tint).toBe(0);
  });
});

describe("sampleNeutral", () => {
  const redPx = [255, 30, 30, 255];

  it("averages a 3×3 window around the click point", () => {
    // 4×4 image, all warm-orange except one green pixel at (2,1).
    const data = new Uint8ClampedArray(4 * 4 * 4);
    for (let i = 0; i < 16; i++) data.set(redPx, i * 4);
    data.set([30, 255, 30, 255], (1 * 4 + 2) * 4);
    const [r, g, b] = sampleNeutral(data, 4, 4, 2, 1);
    // 8 red-ish pixels + 1 green pixel, alpha dropped.
    expect(r).toBeGreaterThan(g);
    expect(g).toBeGreaterThan(b);
  });

  it("clamps the window at the image edges", () => {
    const data = new Uint8ClampedArray(4 * 4);
    for (let i = 0; i < 4; i++) data.set(redPx, i * 4);
    const [r, g] = sampleNeutral(data, 2, 2, 0, 0);
    // Only the 2×2 in-bounds corner of the 3×3 window is averaged.
    expect(r).toBe(1);
    expect(g).toBeCloseTo(30 / 255);
  });

  it("returns normalized [0, 1] channels", () => {
    const data = new Uint8ClampedArray([
      128, 128, 128, 255, 128, 128, 128, 255, 128, 128, 128, 255, 128, 128, 128,
      255,
    ]);
    const [r, g, b] = sampleNeutral(data, 2, 2, 1, 1);
    expect(r).toBeCloseTo(128 / 255);
    expect(g).toBeCloseTo(128 / 255);
    expect(b).toBeCloseTo(128 / 255);
  });
});
