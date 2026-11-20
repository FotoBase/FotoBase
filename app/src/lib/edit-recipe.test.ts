// TASK-04-010 tests: edit-view recipe state. The edit view keeps one state
// object per tab, folds them into a single v1 recipe (edit-recipe spec §1–3)
// in canonical op order, drives the undo/redo history, and rebuilds panel
// states from a stored recipe so reopening an edited photo restores the
// sliders. Pure module — no Svelte, no DOM.

import { describe, expect, it } from "vitest";

import { DEFAULT_TUNING, type TuningState } from "./tuning";
import {
  EMPTY_EDIT_STATE,
  editHistory,
  isEmptyState,
  opsFromEditState,
  pushEditState,
  recipeFromState,
  redo,
  stateFromRecipe,
  undo,
  type EditState,
} from "./edit-recipe";

const untouched: EditState = {
  ...EMPTY_EDIT_STATE,
  tuning: { ...DEFAULT_TUNING },
};

describe("opsFromEditState", () => {
  it("an untouched state emits no ops", () => {
    expect(opsFromEditState(untouched)).toEqual([]);
  });

  it("folds every tab's ops in canonical order (crop first, effects last)", () => {
    const state: EditState = {
      ...untouched,
      crop: {
        crop: { x: 0.1, y: 0.1, w: 0.8, h: 0.8 },
        angle: 0,
        autoFill: true,
      },
      autoFix: { autoColor: true, autoContrast: true },
      tuning: { ...DEFAULT_TUNING, fillLight: 0.4, temperature: -0.2 },
      effects: { bw: 1 },
    };
    const types = opsFromEditState(state).map((op) => op.type);
    expect(types).toEqual([
      "crop",
      "auto_color",
      "auto_contrast",
      "fill_light",
      "temperature",
      "bw",
    ]);
  });

  it("omits tabs at their neutral positions", () => {
    const state: EditState = { ...untouched, effects: { sharpen: 0.5 } };
    expect(opsFromEditState(state)).toEqual([
      { type: "sharpen", params: { amount: 0.5 } },
    ]);
  });
});

describe("recipeFromState", () => {
  it("emits the v1 envelope with the folded ops", () => {
    const state: EditState = { ...untouched, effects: { sepia: 1 } };
    expect(recipeFromState(state)).toEqual({
      spec: "fotobase.recipe",
      version: 1,
      ops: [{ type: "sepia", params: { amount: 1 } }],
    });
  });
});

describe("stateFromRecipe", () => {
  it("round-trips through recipeFromState", () => {
    const state: EditState = {
      ...untouched,
      crop: {
        crop: { x: 0, y: 0, w: 0.5, h: 0.5 },
        angle: 2.5,
        autoFill: false,
      },
      tuning: { ...DEFAULT_TUNING, highlights: 0.7, tint: 0.1 },
      effects: { saturation: -0.5 },
    };
    const restored = stateFromRecipe(recipeFromState(state));
    expect(restored).toEqual(state);
  });

  it("an empty recipe restores the untouched state", () => {
    expect(
      stateFromRecipe({ spec: "fotobase.recipe", version: 1, ops: [] }),
    ).toEqual(untouched);
  });

  it("ignores ops the tabs do not own (text) instead of failing", () => {
    const restored = stateFromRecipe({
      spec: "fotobase.recipe",
      version: 1,
      ops: [
        { type: "text", params: {} },
        { type: "bw", params: { amount: 0.8 } },
      ],
    });
    expect(restored.effects).toEqual({ bw: 0.8 });
  });

  it("owns retouch: strokes are restored and re-emitted in a single op (TASK-08-002)", () => {
    const strokes = [
      { points: [[0.2, 0.3]], radius: 0.03 },
      { points: [[0.5, 0.5], [0.6, 0.5]], radius: 0.05 },
    ];
    const restored = stateFromRecipe({
      ops: [{ type: "retouch", params: { strokes } }],
    });
    expect(restored.retouch.strokes).toEqual(strokes);
    expect(restored.extra).toEqual([]);
    expect(isEmptyState(restored)).toBe(false);
    expect(opsFromEditState(restored)).toEqual([
      { type: "retouch", params: { strokes } },
    ]);
  });

  it("retouch heals the source pixels: it comes before geometry ops", () => {
    // Strokes are drawn on the uncropped preview, so they must apply
    // before crop/straighten for the coordinates to mean the same pixels.
    const state: EditState = {
      ...untouched,
      crop: {
        crop: { x: 0.1, y: 0.1, w: 0.8, h: 0.8 },
        angle: 0,
        autoFill: true,
      },
      retouch: { strokes: [{ points: [[0.5, 0.5]], radius: 0.03 }] },
      effects: { bw: 1 },
    };
    expect(opsFromEditState(state).map((op) => op.type)).toEqual([
      "retouch",
      "crop",
      "bw",
    ]);
  });

  it("owns glyph-renderer text: restored, re-emitted after effects, before frames (TASK-08-003)", () => {
    const text = {
      algo: 2,
      text: "Hi",
      x: 0.3,
      y: 0.4,
      size: 0.1,
      color: "#ffffff",
      font: "sans",
      anchor: "center",
      transparency: 1,
    };
    const restored = stateFromRecipe({
      ops: [
        { type: "text", params: text },
        { type: "border", params: { amount: 0.5 } },
        { type: "bw", params: { amount: 1 } },
      ],
    });
    expect(restored.text.overlays).toHaveLength(1);
    expect(restored.extra).toEqual([]);
    // Text lands on the photo, so frames (which grow the canvas) go last.
    expect(opsFromEditState(restored).map((op) => op.type)).toEqual([
      "bw",
      "text",
      "border",
    ]);
  });

  it("legacy text ops (no algo) stay untouched in extra", () => {
    const legacy = { type: "text", params: { text: "Old", x: 0.1, y: 0.1 } };
    const restored = stateFromRecipe({ ops: [legacy] });
    expect(restored.text.overlays).toEqual([]);
    expect(restored.extra).toEqual([legacy]);
  });

  it("keeps ops no tab owns so a save re-emits them (batch edits survive)", () => {
    // A recipe the batch flow (TASK-04-011) wrote: rotate + a text overlay.
    const batched = {
      spec: "fotobase.recipe",
      version: 1,
      ops: [
        { type: "rotate", params: { angle: 90 } },
        { type: "text", params: { text: "Caption", x: 0.05, y: 0.05 } },
        { type: "film_grain", params: { amount: 0.5, seed: 3 } },
      ],
    };
    const restored = stateFromRecipe(batched);
    expect(restored.extra).toEqual(batched.ops);
    expect(isEmptyState(restored)).toBe(false);

    // Saving from the edit view must not silently drop those edits.
    expect(recipeFromState(restored).ops).toEqual(batched.ops);
  });

  it("extra ops ride along with tab edits in canonical order (tabs first)", () => {
    const state: EditState = {
      ...stateFromRecipe({
        ops: [{ type: "rotate", params: { angle: 90 } }],
      }),
      effects: { bw: 1 },
    };
    expect(opsFromEditState(state)).toEqual([
      { type: "bw", params: { amount: 1 } },
      { type: "rotate", params: { angle: 90 } },
    ]);
  });
});

describe("isEmptyState", () => {
  it("true for the untouched state, false once any tab moves", () => {
    expect(isEmptyState(untouched)).toBe(true);
    expect(isEmptyState({ ...untouched, effects: { bw: 1 } })).toBe(false);
  });
});

describe("editHistory", () => {
  it("starts on the untouched state with nothing to undo or redo", () => {
    const h = editHistory(untouched);
    expect(h.present).toBe(untouched);
    expect(undo(h)).toBeNull();
    expect(redo(h)).toBeNull();
  });

  it("undo steps back through pushed states, redo steps forward", () => {
    let h = editHistory(untouched);
    const second: EditState = { ...untouched, effects: { bw: 1 } };
    const third: EditState = {
      ...untouched,
      tuning: { ...DEFAULT_TUNING, fillLight: 0.5 },
    };
    h = pushEditState(h, second);
    h = pushEditState(h, third);
    expect(h.present).toBe(third);
    h = undo(h)!;
    expect(h.present).toBe(second);
    h = undo(h)!;
    expect(h.present).toBe(untouched);
    expect(undo(h)).toBeNull();
    h = redo(h)!;
    expect(h.present).toBe(second);
  });

  it("a new edit after undo discards the redo branch", () => {
    let h = editHistory(untouched);
    h = pushEditState(h, { ...untouched, effects: { bw: 1 } });
    h = undo(h)!;
    h = pushEditState(h, { ...untouched, effects: { sepia: 1 } });
    expect(redo(h)).toBeNull();
    expect(h.present.effects).toEqual({ sepia: 1 });
  });

  it("pushing an identical state is a no-op", () => {
    let h = editHistory(untouched);
    h = pushEditState(h, untouched);
    expect(undo(h)).toBeNull();
  });
});

// TASK-07-007: auto-rotate (AI-estimated leveling) is a first-class state
// field, emitted as the v1 `auto_rotate` op and restored on reopen.
describe("auto_rotate", () => {
  it("an autoRotate angle emits an auto_rotate op in the geometry group", () => {
    const state: EditState = { ...untouched, autoRotate: -5 };
    expect(opsFromEditState(state)).toEqual([
      { type: "auto_rotate", params: { angle_deg: -5 } },
    ]);
  });

  it("no estimate means no op", () => {
    expect(opsFromEditState({ ...untouched, autoRotate: null })).toEqual([]);
    expect(isEmptyState({ ...untouched, autoRotate: null })).toBe(true);
  });

  it("round-trips through recipeFromState / stateFromRecipe", () => {
    const state: EditState = { ...untouched, autoRotate: 3.5 };
    const restored = stateFromRecipe(recipeFromState(state));
    expect(restored.autoRotate).toBeCloseTo(3.5);
  });

  it("a stored auto_rotate op restores the angle", () => {
    const restored = stateFromRecipe({
      spec: "fotobase.recipe",
      version: 1,
      ops: [{ type: "auto_rotate", params: { angle_deg: -2.5 } }],
    });
    expect(restored.autoRotate).toBeCloseTo(-2.5);
  });

  it("composes with the manual straighten slider", () => {
    const state: EditState = {
      ...untouched,
      autoRotate: -5,
      crop: { crop: { x: 0, y: 0, w: 1, h: 1 }, angle: 2, autoFill: true },
    };
    const types = opsFromEditState(state).map((op) => op.type);
    expect(types).toEqual(["straighten", "auto_rotate"]);
  });
});

// TASK-08-001: the new effects and frames round-trip through the recipe and
// keep working with undo/redo.
describe("remaining effects and frames", () => {
  const effects = {
    warmify: 0.5,
    tint: 0.4,
    film_grain: 0.3,
    soft_focus: 0.6,
    vignette: 0.7,
    border: 0.5,
    drop_shadow: 0.8,
  };

  it("emit in canonical order after tuning, frames last", () => {
    const state: EditState = {
      ...untouched,
      tuning: { ...DEFAULT_TUNING, tint: 0.2 },
      effects,
    };
    expect(opsFromEditState(state).map((o) => o.type)).toEqual([
      "tint", // Tuning-tab white-balance tint
      "warmify",
      "tint", // Effects-tab color tint
      "film_grain",
      "soft_focus",
      "vignette",
      "border",
      "drop_shadow",
    ]);
  });

  it("round-trip through recipeFromState / stateFromRecipe", () => {
    const state: EditState = {
      ...untouched,
      tuning: { ...DEFAULT_TUNING, tint: 0.2 },
      effects,
    };
    expect(stateFromRecipe(recipeFromState(state))).toEqual(state);
  });

  it("a colored tint never leaks into the Tuning tint slider", () => {
    const restored = stateFromRecipe({
      spec: "fotobase.recipe",
      version: 1,
      ops: [{ type: "tint", params: { color: "#ff0000", amount: 0.5 } }],
    });
    expect(restored.tuning.tint).toBe(DEFAULT_TUNING.tint);
    // Custom color: not the tab's own op, so it is preserved verbatim.
    expect(restored.extra).toEqual([
      { type: "tint", params: { color: "#ff0000", amount: 0.5 } },
    ]);
  });

  it("undo/redo step through effect changes", () => {
    let h = editHistory(untouched);
    h = pushEditState(h, { ...untouched, effects: { vignette: 0.5 } });
    h = pushEditState(h, {
      ...untouched,
      effects: { vignette: 0.5, border: 0.5 },
    });
    h = undo(h)!;
    expect(h.present.effects).toEqual({ vignette: 0.5 });
    h = redo(h)!;
    expect(h.present.effects).toEqual({ vignette: 0.5, border: 0.5 });
  });
});
