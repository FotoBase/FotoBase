// TASK-04-009 tests: starter effects state → recipe ops. Sharpen, Sepia,
// B&W and Saturation are one-click recipe ops with a strength param;
// strength 0 emits no op (the buffer must stay untouched).

import { describe, expect, it } from "vitest";

import {
  DEFAULT_STRENGTH,
  clampedStrength,
  opsFromEffects,
  toggleEffect,
  type EffectsState,
} from "./effects";

describe("effects", () => {
  it("an empty state emits no ops", () => {
    expect(opsFromEffects({})).toEqual([]);
  });

  it("toggling an effect emits its op at the default strength", () => {
    const state = toggleEffect({}, "bw");
    expect(opsFromEffects(state)).toEqual([
      { type: "bw", params: { amount: DEFAULT_STRENGTH.bw } },
    ]);
  });

  it("toggling an active effect removes it from the recipe", () => {
    const on = toggleEffect({}, "sepia");
    expect(opsFromEffects(on)).toHaveLength(1);
    const off = toggleEffect(on, "sepia");
    expect(opsFromEffects(off)).toEqual([]);
  });

  it("strength 0 emits no op for the effect (buffer unchanged)", () => {
    const state = { sharpen: 0, sepia: 0, bw: 0, saturation: 0 };
    expect(opsFromEffects(state)).toEqual([]);
  });

  it("each effect emits its op with its strength as amount", () => {
    const state: EffectsState = {
      sharpen: 0.8,
      sepia: 0.4,
      bw: 1,
      saturation: -0.2,
    };
    expect(opsFromEffects(state)).toEqual([
      { type: "sharpen", params: { amount: 0.8 } },
      { type: "sepia", params: { amount: 0.4 } },
      { type: "bw", params: { amount: 1 } },
      { type: "saturation", params: { amount: -0.2 } },
    ]);
  });

  it("strength is clamped to the op's documented param range", () => {
    expect(clampedStrength("sharpen", 2)).toBe(1);
    expect(clampedStrength("sharpen", -1)).toBe(0);
    // Saturation may go negative (desaturate) down to -1.
    expect(clampedStrength("saturation", -2)).toBe(-1);
  });

  it("every effect has a documented default strength", () => {
    for (const effect of ["sharpen", "sepia", "bw", "saturation"] as const) {
      expect(DEFAULT_STRENGTH[effect]).toBeGreaterThan(0);
    }
  });
});
