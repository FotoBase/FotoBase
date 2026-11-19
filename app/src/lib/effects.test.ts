// TASK-04-009 tests: starter effects state → recipe ops. Sharpen, Sepia,
// B&W and Saturation are one-click recipe ops with a strength param;
// strength 0 emits no op (the buffer must stay untouched).

import { describe, expect, it } from "vitest";

import {
  DEFAULT_STRENGTH,
  clampedStrength,
  EFFECTS,
  effectFromOp,
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

// TASK-08-001: Warmify, Film Grain, Tint, Soft Focus, Vignette and the
// Border / Drop Shadow frames join the Effects tab.
describe("remaining effects and frames", () => {
  const NEW = [
    "warmify",
    "tint",
    "film_grain",
    "soft_focus",
    "vignette",
    "border",
    "drop_shadow",
  ] as const;

  it("every new effect has a one-click default strength", () => {
    for (const effect of NEW) {
      expect(EFFECTS).toContain(effect);
      expect(DEFAULT_STRENGTH[effect]).toBeGreaterThan(0);
      expect(clampedStrength(effect, 5)).toBe(1);
      expect(clampedStrength(effect, -5)).toBe(0);
    }
  });

  it("one click emits the op with its default strength", () => {
    for (const effect of NEW) {
      const ops = opsFromEffects(toggleEffect({}, effect));
      expect(ops).toHaveLength(1);
      expect(ops[0].type).toBe(effect);
      expect(ops[0].params.amount).toBe(DEFAULT_STRENGTH[effect]);
    }
  });

  it("tint emits a color so it is never read as the white-balance tint", () => {
    const [op] = opsFromEffects({ tint: 0.4 });
    expect(op.params).toEqual({
      amount: 0.4,
      color: expect.stringMatching(/^#[0-9a-f]{6}$/i),
    });
  });

  it("film grain emits the same params the batch flow writes", () => {
    expect(opsFromEffects({ film_grain: 0.5 })[0].params).toEqual({
      amount: 0.5,
      seed: 0,
    });
  });

  it("frames come last so photo effects never act on the frame", () => {
    const state: EffectsState = {
      drop_shadow: 0.5,
      border: 0.5,
      vignette: 0.5,
      sharpen: 0.5,
      film_grain: 0.5,
    };
    expect(opsFromEffects(state).map((o) => o.type)).toEqual([
      "sharpen",
      "film_grain",
      "vignette",
      "border",
      "drop_shadow",
    ]);
  });

  it("effectFromOp only claims ops the tab would emit itself", () => {
    expect(effectFromOp("vignette", { amount: 0.3 })).toEqual([
      "vignette",
      0.3,
    ]);
    expect(effectFromOp("film_grain", { amount: 0.5, seed: 0 })).toEqual([
      "film_grain",
      0.5,
    ]);
    // Foreign seed / custom colors / white-balance tint belong elsewhere.
    expect(effectFromOp("film_grain", { amount: 0.5, seed: 3 })).toBeNull();
    expect(
      effectFromOp("border", { amount: 0.5, color: "#ff0000" }),
    ).toBeNull();
    expect(effectFromOp("tint", { green_magenta: 0.2 })).toBeNull();
    expect(effectFromOp("temperature", { kelvin_shift: 0.2 })).toBeNull();
  });
});
